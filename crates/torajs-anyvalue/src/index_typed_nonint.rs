//! The keyed read behind a typed array's non-integral index — the
//! slow leg of torajs-arr's `__torajs_arr_typed_nonint_{f64,str}`,
//! reached through the `_slow` seams (defaults in `torajs-dispatch`)
//! only for a cell that can carry a named property.

use core::ffi::c_void;

use crate::index_any_keyed::__torajs_any_index_get_keyed;
use crate::nanbox::{is_cell, is_short_str, is_undefined};

/// `number[]` flavor: §7.1.19 names the property "1.5"; `undefined`
/// answers the F64 slot's sentinel and anything else goes through
/// ToNumber (the lane's recorded approximation for a non-number
/// expando).
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_f64_impl(arr: *mut c_void, key: f64) -> f64 {
    unsafe {
        let v = __torajs_any_index_get_keyed(
            crate::nanbox::box_void_ptr(arr),
            crate::nanbox::box_double(key),
        );
        if is_undefined(v) {
            return f64::from_bits(torajs_rc::F64_UNDEF_SENTINEL_BITS);
        }
        let n = crate::nanbox_ffi::__torajs_anyv_to_number(v);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
        n
    }
}

/// `string[]` flavor: a cell (or inline short string) answer is
/// handed over as the caller's own stake through the owned unbox;
/// `undefined` and the other immediates answer NULL, which the
/// lowering reads as the Str slot's undefined cell.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_str_impl(
    arr: *mut c_void,
    key: f64,
) -> *mut c_void {
    unsafe {
        let v = __torajs_any_index_get_keyed(
            crate::nanbox::box_void_ptr(arr),
            crate::nanbox::box_double(key),
        );
        let out = if is_short_str(v) || is_cell(v) {
            crate::nanbox_encode::__torajs_anyv_unbox_value_owned(v) as *mut c_void
        } else {
            core::ptr::null_mut()
        };
        crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
        out
    }
}

/// `boolean[]` / pointer-element flavor: the answer as it is, owned,
/// when `keep != 0`; otherwise the answer is released and `undefined`
/// stands in — a pointer slot has no spelling for a value of another
/// type, so only the read's getter (and its throw) shows.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_typed_nonint_any_impl(
    arr: *mut c_void,
    key: f64,
    keep: i64,
) -> u64 {
    unsafe {
        let v = __torajs_any_index_get_keyed(
            crate::nanbox::box_void_ptr(arr),
            crate::nanbox::box_double(key),
        );
        if keep != 0 {
            return v;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
        crate::nanbox::VALUE_UNDEFINED
    }
}
