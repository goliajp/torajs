//! Shared substrate for `Iterator.zip` / `Iterator.zipKeyed` — RFC
//! 20260730-iterator-global 刀 5b/5c (proposal-joint-iteration).
//!
//! Both statics parse the same options bag (mode / longest-mode
//! padding), keep their open columns in an `Array<Any>` whose
//! exhausted slots overwrite to undefined, and drive the same
//! three-mode row step — they differ only in how a row materializes
//! (array vs key-tagged object), which [`RowSink`] abstracts.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_drop_any(arr: *mut c_void);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Array length word (mirror of `torajs-arr::layout::ARR_LEN_OFF`).
pub(crate) const ARR_LEN_OFF: usize = 8;
/// B1 data pointer slot (mirror of `layout::ARR_DATA_PTR_OFF`).
pub(crate) const ARR_DATA_PTR_OFF: usize = 32;
/// torajs-str Str layout — u64 length at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
const STR_HDR_SIZE: usize = 16;

/// Member-protocol slot tags.
pub(crate) const TAG_UNDEF: u64 = 5;
pub(crate) const TAG_HEAP: u64 = 4;

/// Modes (the cell's counter word).
pub(crate) const ZIP_MODE_SHORTEST: u64 = 0;
pub(crate) const ZIP_MODE_LONGEST: u64 = 1;
pub(crate) const ZIP_MODE_STRICT: u64 = 2;

/// True when `v` is a language-value Object (a heap cell that is not
/// a primitive Str / Symbol / BigInt cell).
pub(crate) unsafe fn av_is_object(v: AnyValue) -> bool {
    if is_short_str(v) || !is_cell(v) {
        return false;
    }
    let t = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    t != Tag::Str as u16 && t != Tag::Symbol as u16 && t != Tag::BigInt as u16
}

/// Compare a string-family AnyValue against ASCII `expect`. Only
/// called after the string-family gate; materializes through
/// ToString (no coercion side effects on an actual string).
unsafe fn str_av_eq(v: AnyValue, expect: &[u8]) -> bool {
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(v);
        if s.is_null() {
            return false;
        }
        let len = (s.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() as usize;
        let eq = len == expect.len()
            && core::slice::from_raw_parts(s.cast::<u8>().add(STR_HDR_SIZE), len) == expect;
        __torajs_str_drop(s as *mut c_void);
        eq
    }
}

/// True when `v` sits in the string family (ShortStr immediate or a
/// Str-tagged cell — Substr views share the tag).
unsafe fn av_is_string(v: AnyValue) -> bool {
    if is_short_str(v) {
        return true;
    }
    is_cell(v)
        && unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() } == Tag::Str as u16
}

/// Borrowed member read `(tag, payload)` off an object, string key.
pub(crate) unsafe fn member_pair(obj: AnyValue, name: &[u8]) -> (u64, u64) {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        let pair = member_pair_cell(obj, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        pair
    }
}

/// Borrowed member read with an existing key cell.
pub(crate) unsafe fn member_pair_cell(obj: AnyValue, key: *const c_void) -> (u64, u64) {
    unsafe {
        let tag = crate::member_get::__torajs_any_member_get_tag(obj, key);
        let payload = if tag == TAG_UNDEF {
            0
        } else {
            crate::member_get_value::__torajs_any_member_get_value(obj, key)
        };
        (tag, payload)
    }
}

/// GetOptionsObject + mode + (longest) padding-option — the shared
/// head of both statics. `None` = pending throw.
pub(crate) unsafe fn zip_parse_options(options: AnyValue) -> Option<(u64, AnyValue)> {
    unsafe {
        if options != VALUE_UNDEFINED && !av_is_object(options) {
            __torajs_throw_type_error(c"Iterator.zip options is not an object".as_ptr());
            return None;
        }
        let mut mode = ZIP_MODE_SHORTEST;
        if options != VALUE_UNDEFINED {
            let (t, p) = member_pair(options, b"mode");
            if t != TAG_UNDEF {
                let mode_av = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, p as i64);
                if !av_is_string(mode_av) {
                    __torajs_throw_type_error(c"Iterator.zip mode is invalid".as_ptr());
                    return None;
                }
                mode = if str_av_eq(mode_av, b"shortest") {
                    ZIP_MODE_SHORTEST
                } else if str_av_eq(mode_av, b"longest") {
                    ZIP_MODE_LONGEST
                } else if str_av_eq(mode_av, b"strict") {
                    ZIP_MODE_STRICT
                } else {
                    __torajs_throw_type_error(c"Iterator.zip mode is invalid".as_ptr());
                    return None;
                };
            }
        }
        let mut padding_opt = VALUE_UNDEFINED;
        if mode == ZIP_MODE_LONGEST && options != VALUE_UNDEFINED {
            let (t, p) = member_pair(options, b"padding");
            if t != TAG_UNDEF {
                let pad_av = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, p as i64);
                if !av_is_object(pad_av) {
                    __torajs_throw_type_error(c"Iterator.zip padding is not an object".as_ptr());
                    return None;
                }
                padding_opt = pad_av;
            }
        }
        Some((mode, padding_opt))
    }
}

/// Close every still-open iterator in `iters` except `skip` (pass
/// `u64::MAX` to close all). Slots keep their references — the cell
/// drop glue (or the caller's explicit drop) releases them.
pub(crate) unsafe fn close_open_iters(iters: *const c_void, count: u64, skip: u64) {
    unsafe {
        for j in 0..count {
            if j == skip {
                continue;
            }
            let av = __torajs_arr_get_any_boxed(iters, j);
            if av != VALUE_UNDEFINED {
                crate::iter_any_close::__torajs_iter_close_value(av);
            }
        }
    }
}

/// One row under construction — an `Array<Any>` for zip, a fresh
/// dynobj keyed off the mint-time keys snapshot for zipKeyed. `put`
/// takes the `(tag, value)` pair OWNED (push / set transfer, exactly
/// one consumer per pair).
pub(crate) enum RowSink {
    Arr(*mut c_void),
    /// `(row dynobj, keys Arr<Str> cell)` — the key for column `i`
    /// reads from the typed keys array's data slots.
    Obj(*mut c_void, *const c_void),
}

impl RowSink {
    pub(crate) unsafe fn put(&mut self, i: u64, tag: u64, value: u64) {
        unsafe {
            match self {
                RowSink::Arr(arr) => {
                    *arr = __torajs_arr_push_any(*arr, tag, value) as *mut c_void;
                }
                RowSink::Obj(obj, keys) => {
                    let data =
                        ((*keys as *const u8).add(ARR_DATA_PTR_OFF) as *const *const u8).read();
                    let key = (data.add(i as usize * 8) as *const *mut c_void).read();
                    __torajs_dynobj_set(obj, key, tag, value);
                }
            }
        }
    }

    /// The finished row as an owned AnyValue.
    pub(crate) unsafe fn finish(self) -> AnyValue {
        unsafe {
            match self {
                RowSink::Arr(arr) => crate::nanbox_encode::__torajs_anyv_box_pointer(arr),
                RowSink::Obj(obj, _) => obj as u64,
            }
        }
    }

    /// Abandon a half-built row (done / abrupt exits).
    pub(crate) unsafe fn abandon(self) {
        unsafe {
            match self {
                RowSink::Arr(arr) => __torajs_arr_drop_any(arr),
                RowSink::Obj(obj, _) => __torajs_value_drop_heap(obj),
            }
        }
    }
}
