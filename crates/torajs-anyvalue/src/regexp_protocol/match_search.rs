//! §22.2.6.8 `RegExp.prototype[@@match]` and §22.2.6.12
//! `RegExp.prototype[@@search]`.

use core::ffi::c_void;

use super::ops::{
    ABRUPT, Owned, advance_string_index, box_number, flags_of, get, get_index, has_flag,
    last_index_length, regexp_exec, set_last_index, to_string,
};
use crate::nanbox::{AnyValue, VALUE_NULL, box_int32, is_null};

unsafe extern "C" {
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
}

/// §22.2.6.8 steps 3-6.
pub(super) unsafe fn symbol_match(rx: AnyValue, string: AnyValue) -> AnyValue {
    unsafe {
        let Some(s) = to_string(string) else {
            return ABRUPT;
        };
        let Some(flags) = flags_of(rx) else {
            return ABRUPT;
        };
        if !has_flag(&flags, b'g') {
            return regexp_exec(rx, &s).map_or(ABRUPT, Owned::take);
        }
        let full_unicode = has_flag(&flags, b'u') || has_flag(&flags, b'v');
        if set_last_index(rx, box_int32(0)).is_none() {
            return ABRUPT;
        }
        let units = s.units();
        let mut a = Owned::adopt(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            4,
            __torajs_arr_alloc_any(0) as i64,
        ));
        let mut n = 0usize;
        loop {
            let Some(result) = regexp_exec(rx, &s) else {
                return ABRUPT;
            };
            if is_null(result.get()) {
                return if n == 0 { VALUE_NULL } else { a.take() };
            }
            let Some(m0) = get_index(result.get(), 0) else {
                return ABRUPT;
            };
            let Some(match_str) = to_string(m0.get()) else {
                return ABRUPT;
            };
            let empty = match_str.is_empty();
            let arr = a.take();
            let grown = __torajs_arr_push_any(
                crate::nanbox::as_void_ptr(arr),
                4,
                match_str.into_raw() as u64,
            );
            a = Owned::adopt(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                4,
                grown as i64,
            ));
            if empty {
                let Some(this_index) = last_index_length(rx) else {
                    return ABRUPT;
                };
                let next = advance_string_index(&units, this_index, full_unicode);
                if set_last_index(rx, box_number(next as f64)).is_none() {
                    return ABRUPT;
                }
            }
            n += 1;
        }
    }
}

/// §22.2.6.12 steps 3-10.
pub(super) unsafe fn symbol_search(rx: AnyValue, string: AnyValue) -> AnyValue {
    unsafe {
        let Some(s) = to_string(string) else {
            return ABRUPT;
        };
        let Some(previous) = get(rx, b"lastIndex") else {
            return ABRUPT;
        };
        let zero = box_int32(0);
        if !crate::nanbox_ffi::__torajs_anyv_same_value(previous.get(), zero)
            && set_last_index(rx, zero).is_none()
        {
            return ABRUPT;
        }
        let Some(result) = regexp_exec(rx, &s) else {
            return ABRUPT;
        };
        let Some(current) = get(rx, b"lastIndex") else {
            return ABRUPT;
        };
        if !crate::nanbox_ffi::__torajs_anyv_same_value(current.get(), previous.get())
            && set_last_index(rx, previous.get()).is_none()
        {
            return ABRUPT;
        }
        if is_null(result.get()) {
            return box_int32(-1);
        }
        get(result.get(), b"index").map_or(ABRUPT, Owned::take)
    }
}
