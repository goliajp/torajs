//! `Iterator.zip(iterables, options)` — RFC 20260730-iterator-global
//! 刀 5b (proposal-joint-iteration, stage 3).
//!
//! The static opens EVERY input iterator eagerly (GetIteratorFlattenable
//! with REJECT-STRINGS over each element of `iterables`; an abrupt
//! open closes everything already open) and answers a lazy kind-ZIP
//! IterHelper cell:
//!
//! - underlying = the open iterators `Array<Any>` — an exhausted
//!   slot is overwritten with undefined (`arr_set_any` drops the
//!   cell) so "still open" is exactly "slot is not undefined";
//! - fn        = the padding `Array<Any>` (longest mode; undefined
//!   otherwise);
//! - counter   = the mode (0 shortest / 1 longest / 2 strict);
//! - inner     = unused (undefined).
//!
//! Each step drives every open iterator once, per the proposal's
//! IteratorZip closure: shortest finishes (closing the others) on the
//! first done; longest substitutes `padding[i]` and finishes when no
//! iterator remains open; strict requires column 0 to finish first
//! and every sibling to agree.

use core::ffi::c_void;

use crate::iter_from::derive_flattenable;
use crate::iter_helper::{
    ALIVE_OFF, COUNTER_OFF, FN_OFF, ITER_HELPER_ZIP, UNDERLYING_OFF, iter_helper_cell_alloc,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// In-bounds slot overwrite — drops the previous heap value and
    /// takes the new pair WITHOUT inc (transfer).
    fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64);
    fn __torajs_arr_drop_any(arr: *mut c_void);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Array length word (mirror of `torajs-arr::layout::ARR_LEN_OFF`).
const ARR_LEN_OFF: usize = 8;
/// torajs-str Str layout — u64 length at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
const STR_HDR_SIZE: usize = 16;

/// Member-protocol slot tags.
const TAG_UNDEF: u64 = 5;
const TAG_HEAP: u64 = 4;

/// Modes (the cell's counter word).
pub(crate) const ZIP_MODE_SHORTEST: u64 = 0;
pub(crate) const ZIP_MODE_LONGEST: u64 = 1;
pub(crate) const ZIP_MODE_STRICT: u64 = 2;

/// True when `v` is a language-value Object (a heap cell that is not
/// a primitive Str / Symbol / BigInt cell).
unsafe fn av_is_object(v: AnyValue) -> bool {
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
        let len = (s.cast::<u8>().add(STR_LEN_OFF) as *const u64).read() as usize;
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

/// Borrowed member read `(tag, payload)` off an options object.
unsafe fn member_pair(obj: AnyValue, name: &[u8]) -> (u64, u64) {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        let tag = crate::member_get::__torajs_any_member_get_tag(obj, key as *const c_void);
        let payload = if tag == TAG_UNDEF {
            0
        } else {
            crate::member_get_value::__torajs_any_member_get_value(obj, key as *const c_void)
        };
        __torajs_str_drop(key as *mut c_void);
        (tag, payload)
    }
}

/// Close every still-open iterator in `iters` except `skip` (pass
/// `u64::MAX` to close all). Slots keep their references — the cell
/// drop glue (or the caller's explicit drop) releases them.
unsafe fn close_open_iters(iters: *const c_void, count: u64, skip: u64) {
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

/// `Iterator.zip(iterables, options)` kernel. Validates options,
/// opens every input iterator eagerly, collects longest-mode padding,
/// and mints the kind-ZIP cell. Undefined with a pending throw on any
/// refusal (everything already open is closed per
/// IfAbruptCloseIterators).
///
/// # Safety
/// `iterables` / `options` carry valid AnyValue bit patterns
/// (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_zip(iterables: AnyValue, options: AnyValue) -> AnyValue {
    unsafe {
        // Step 1 — iterables must be an Object.
        if !av_is_object(iterables) {
            __torajs_throw_type_error(c"Iterator.zip iterables is not an object".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Step 2 — GetOptionsObject.
        if options != VALUE_UNDEFINED && !av_is_object(options) {
            __torajs_throw_type_error(c"Iterator.zip options is not an object".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Steps 3-5 — mode.
        let mut mode = ZIP_MODE_SHORTEST;
        if options != VALUE_UNDEFINED {
            let (t, p) = member_pair(options, b"mode");
            if t != TAG_UNDEF {
                let mode_av = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, p as i64);
                if !av_is_string(mode_av) {
                    __torajs_throw_type_error(c"Iterator.zip mode is invalid".as_ptr());
                    return VALUE_UNDEFINED;
                }
                mode = if str_av_eq(mode_av, b"shortest") {
                    ZIP_MODE_SHORTEST
                } else if str_av_eq(mode_av, b"longest") {
                    ZIP_MODE_LONGEST
                } else if str_av_eq(mode_av, b"strict") {
                    ZIP_MODE_STRICT
                } else {
                    __torajs_throw_type_error(c"Iterator.zip mode is invalid".as_ptr());
                    return VALUE_UNDEFINED;
                };
            }
        }
        // Step 6 — the padding OPTION reads before the iterables
        // iterate (its own iteration runs after the opens).
        let mut padding_opt = VALUE_UNDEFINED;
        if mode == ZIP_MODE_LONGEST && options != VALUE_UNDEFINED {
            let (t, p) = member_pair(options, b"padding");
            if t != TAG_UNDEF {
                let pad_av = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, p as i64);
                if !av_is_object(pad_av) {
                    __torajs_throw_type_error(c"Iterator.zip padding is not an object".as_ptr());
                    return VALUE_UNDEFINED;
                }
                padding_opt = pad_av;
            }
        }
        // Steps 7-8 — open every input iterator eagerly.
        let Some(outer) = derive_flattenable(iterables, false) else {
            return VALUE_UNDEFINED;
        };
        let mut iters = __torajs_arr_alloc_any(0) as *mut c_void;
        loop {
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_any_step::step_derived_iterator(outer, &mut item, false);
            if hit == 0 {
                if __torajs_throw_check() != 0 {
                    let n = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
                    close_open_iters(iters, n, u64::MAX);
                    __torajs_arr_drop_any(iters);
                    __torajs_anyv_rc_dec(outer);
                    return VALUE_UNDEFINED;
                }
                break;
            }
            let opened = derive_flattenable(item, false);
            __torajs_anyv_rc_dec(item);
            match opened {
                Some(it) => {
                    // Transfer — push takes the reference.
                    iters = __torajs_arr_push_any(iters, TAG_HEAP, it) as *mut c_void;
                }
                None => {
                    crate::iter_any_close::__torajs_iter_close_value(outer);
                    let n = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
                    close_open_iters(iters, n, u64::MAX);
                    __torajs_arr_drop_any(iters);
                    __torajs_anyv_rc_dec(outer);
                    return VALUE_UNDEFINED;
                }
            }
        }
        __torajs_anyv_rc_dec(outer);
        let count = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        // Step 9 — longest-mode padding list, exactly `count` long.
        let mut padding_arr: *mut c_void = core::ptr::null_mut();
        if mode == ZIP_MODE_LONGEST {
            padding_arr = __torajs_arr_alloc_any(count) as *mut c_void;
            let mut filled: u64 = 0;
            if padding_opt != VALUE_UNDEFINED && count > 0 {
                match derive_flattenable(padding_opt, false) {
                    Some(pad_iter) => {
                        while filled < count {
                            let mut item: AnyValue = VALUE_UNDEFINED;
                            let hit = crate::iter_any_step::step_derived_iterator(
                                pad_iter, &mut item, false,
                            );
                            if hit == 0 {
                                if __torajs_throw_check() != 0 {
                                    __torajs_anyv_rc_dec(pad_iter);
                                    close_open_iters(iters, count, u64::MAX);
                                    __torajs_arr_drop_any(iters);
                                    __torajs_arr_drop_any(padding_arr);
                                    return VALUE_UNDEFINED;
                                }
                                break;
                            }
                            // Transfer the stepped value into the slot.
                            let (t, p) = (
                                crate::__torajs_anyv_unbox_tag(item),
                                crate::__torajs_anyv_unbox_value(item),
                            );
                            padding_arr = __torajs_arr_push_any(padding_arr, t as u64, p as u64)
                                as *mut c_void;
                            filled += 1;
                        }
                        if filled == count {
                            crate::iter_any_close::__torajs_iter_close_value(pad_iter);
                        }
                        __torajs_anyv_rc_dec(pad_iter);
                    }
                    None => {
                        close_open_iters(iters, count, u64::MAX);
                        __torajs_arr_drop_any(iters);
                        __torajs_arr_drop_any(padding_arr);
                        return VALUE_UNDEFINED;
                    }
                }
            }
            while filled < count {
                padding_arr = __torajs_arr_push_any(padding_arr, TAG_UNDEF, 0) as *mut c_void;
                filled += 1;
            }
        }
        // Step 10 — mint.
        let cell = iter_helper_cell_alloc(ITER_HELPER_ZIP);
        *(cell.add(UNDERLYING_OFF) as *mut u64) =
            crate::nanbox_encode::__torajs_anyv_box_pointer(iters);
        if !padding_arr.is_null() {
            *(cell.add(FN_OFF) as *mut u64) =
                crate::nanbox_encode::__torajs_anyv_box_pointer(padding_arr);
        }
        *(cell.add(COUNTER_OFF) as *mut u64) = mode;
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// One protocol step of a kind-ZIP cell — the proposal's IteratorZip
/// closure body. 1 with `*out` owning a fresh results `Array<Any>`,
/// 0 on done / pending throw.
///
/// # Safety
/// `ptr` is a live IterHelper cell of kind ZIP (alive already checked
/// by the caller); `out` is writable.
pub(crate) unsafe fn iter_zip_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let p = ptr.cast::<u8>();
        let mode = (p.add(COUNTER_OFF) as *const u64).read();
        let iters_av = (p.add(UNDERLYING_OFF) as *const u64).read();
        let iters = as_void_ptr(iters_av) as *mut c_void;
        let count = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        let padding_av = (p.add(FN_OFF) as *const u64).read();
        if count == 0 {
            (p.add(ALIVE_OFF)).write(0);
            return 0;
        }
        if mode == ZIP_MODE_LONGEST {
            // Done exactly when no iterator remains open.
            let mut any_open = false;
            for i in 0..count {
                if __torajs_arr_get_any_boxed(iters, i) != VALUE_UNDEFINED {
                    any_open = true;
                    break;
                }
            }
            if !any_open {
                (p.add(ALIVE_OFF)).write(0);
                return 0;
            }
        }
        let mut results = __torajs_arr_alloc_any(count) as *mut c_void;
        for i in 0..count {
            let iter_av = __torajs_arr_get_any_boxed(iters, i);
            if iter_av == VALUE_UNDEFINED {
                // Longest — exhausted column reads its padding.
                let pad = __torajs_arr_get_any_boxed(as_void_ptr(padding_av), i);
                let (t, v) = (
                    crate::__torajs_anyv_unbox_tag(pad),
                    crate::__torajs_anyv_unbox_value(pad),
                );
                crate::payload_rc_inc(t, v);
                results = __torajs_arr_push_any(results, t as u64, v as u64) as *mut c_void;
                continue;
            }
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_any_step::step_derived_iterator(iter_av, &mut item, false);
            if hit == 1 {
                let (t, v) = (
                    crate::__torajs_anyv_unbox_tag(item),
                    crate::__torajs_anyv_unbox_value(item),
                );
                results = __torajs_arr_push_any(results, t as u64, v as u64) as *mut c_void;
                continue;
            }
            if __torajs_throw_check() != 0 {
                // Abrupt step — the column is spent (spec removes it
                // from openIters); close the surviving columns, die.
                __torajs_arr_set_any(iters, i, TAG_UNDEF, 0);
                close_open_iters(iters, count, u64::MAX);
                (p.add(ALIVE_OFF)).write(0);
                __torajs_arr_drop_any(results);
                return 0;
            }
            // Column i finished — remove it from the open set (the
            // slot overwrite drops the cell) so the close sweeps
            // below never call return() on a finished iterator.
            __torajs_arr_set_any(iters, i, TAG_UNDEF, 0);
            match mode {
                ZIP_MODE_SHORTEST => {
                    close_open_iters(iters, count, u64::MAX);
                    (p.add(ALIVE_OFF)).write(0);
                    __torajs_arr_drop_any(results);
                    return 0;
                }
                ZIP_MODE_STRICT => {
                    if i != 0 {
                        __torajs_throw_type_error(
                            c"Iterator.zip strict mode: lengths differ".as_ptr(),
                        );
                        close_open_iters(iters, count, u64::MAX);
                        (p.add(ALIVE_OFF)).write(0);
                        __torajs_arr_drop_any(results);
                        return 0;
                    }
                    // Column 0 done first — every sibling must agree.
                    for k in 1..count {
                        let sib = __torajs_arr_get_any_boxed(iters, k);
                        if sib == VALUE_UNDEFINED {
                            continue;
                        }
                        let mut sv: AnyValue = VALUE_UNDEFINED;
                        let shit = crate::iter_any_step::step_derived_iterator(sib, &mut sv, false);
                        if shit == 1 {
                            // Sibling still has values — strict
                            // refuses; it stays open and gets closed
                            // with the rest.
                            __torajs_anyv_rc_dec(sv);
                            __torajs_throw_type_error(
                                c"Iterator.zip strict mode: lengths differ".as_ptr(),
                            );
                            close_open_iters(iters, count, u64::MAX);
                            (p.add(ALIVE_OFF)).write(0);
                            __torajs_arr_drop_any(results);
                            return 0;
                        }
                        // Done (agrees) or abrupt — either way the
                        // column is spent.
                        __torajs_arr_set_any(iters, k, TAG_UNDEF, 0);
                        if __torajs_throw_check() != 0 {
                            close_open_iters(iters, count, u64::MAX);
                            (p.add(ALIVE_OFF)).write(0);
                            __torajs_arr_drop_any(results);
                            return 0;
                        }
                    }
                    (p.add(ALIVE_OFF)).write(0);
                    __torajs_arr_drop_any(results);
                    return 0;
                }
                _ => {
                    // Longest — the column was already retired above.
                    // If it was the LAST open column the row is
                    // abandoned, per the proposal's "openIters is
                    // empty → return ReturnCompletion" (a trailing
                    // all-padding row would over-run the longest
                    // input).
                    let mut any_open = false;
                    for j in 0..count {
                        if __torajs_arr_get_any_boxed(iters, j) != VALUE_UNDEFINED {
                            any_open = true;
                            break;
                        }
                    }
                    if !any_open {
                        (p.add(ALIVE_OFF)).write(0);
                        __torajs_arr_drop_any(results);
                        return 0;
                    }
                    // Substitute the retired column's padding.
                    let pad = __torajs_arr_get_any_boxed(as_void_ptr(padding_av), i);
                    let (t, v) = (
                        crate::__torajs_anyv_unbox_tag(pad),
                        crate::__torajs_anyv_unbox_value(pad),
                    );
                    crate::payload_rc_inc(t, v);
                    results = __torajs_arr_push_any(results, t as u64, v as u64) as *mut c_void;
                }
            }
        }
        *out = crate::nanbox_encode::__torajs_anyv_box_pointer(results);
        1
    }
}

/// Close every still-open column — the return() face and any other
/// early finish.
///
/// # Safety
/// `ptr` is a live IterHelper cell of kind ZIP.
pub(crate) unsafe fn iter_zip_close_all(ptr: *mut c_void) {
    unsafe {
        let p = ptr.cast::<u8>();
        let iters_av = (p.add(UNDERLYING_OFF) as *const u64).read();
        let iters = as_void_ptr(iters_av) as *const c_void;
        let count = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        close_open_iters(iters, count, u64::MAX);
    }
}
