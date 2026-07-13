//! Array-locales walk for `toLocale{Upper,Lower}Case` — ES402
//! CanonicalizeLocaleList (§9.2.1) over a `Tag::Arr` locales
//! argument: every element is validated (RangeError on the first
//! structurally invalid tag, TypeError on a non-string element),
//! then the first element selects the tailoring the torajs-str
//! kernel applies.
//!
//! Lives on the anyvalue side because walking the array requires
//! the kind-aware `__torajs_arr_index_get` + AnyValue decoding;
//! the per-tag validation and the case fold itself are cross-TU
//! calls into torajs-str.
//!
//! Boundary notes (recorded):
//! - tr's dense Arr cannot distinguish a hole from an explicit
//!   `undefined` element — both raise the explicit-undefined
//!   TypeError (spec skips holes via HasProperty).
//! - String wrapper objects / arbitrary objects with a toString
//!   are not walked (non-string elements raise TypeError).

use core::ffi::c_void;

use crate::nanbox::{as_pointer, as_void_ptr, is_cell, is_short_str};
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-arr — kind-aware `arr[idx]`; balanced AnyValue (+1
    /// for cells).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-str — structural language-tag validation; records a
    /// pending RangeError and answers 0 on an invalid tag.
    fn __torajs_str_locale_check(locale: *const u8) -> i64;
    /// torajs-str — the tailored case-fold kernels.
    fn __torajs_str_to_locale_upper(s: *const u8, locale: *const u8) -> *mut u8;
    fn __torajs_str_to_locale_lower(s: *const u8, locale: *const u8) -> *mut u8;
    /// torajs-str — Str drop.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-rc — balanced-cell release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-throw — catchable TypeError raise (TLS pending).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Read the Arr length via the mirrored layout offset (see
/// [`crate::index_any::MIRROR_ARR_LEN_OFF`]).
#[inline]
unsafe fn arr_len(arr: *const c_void) -> i64 {
    unsafe { *(arr.cast::<u8>().add(crate::index_any::MIRROR_ARR_LEN_OFF) as *const u64) as i64 }
}

/// `s.toLocale{Upper,Lower}Case(localeArr)` with an Arr locales
/// argument. Validates every element per CanonicalizeLocaleList,
/// then folds with the first element's tailoring (empty array =
/// host default). On a pending throw the receiver is echoed as the
/// stand-in (the call site's throw check kills the path).
///
/// # Safety
///
/// `s` is a valid Str heap block; `arr` is a valid `Tag::Arr` heap
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_case_arr(
    s: *const u8,
    arr: *const c_void,
    upper: i64,
) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let mut first: *mut u8 = core::ptr::null_mut();
        for i in 0..len {
            let av = __torajs_arr_index_get(arr, i);
            let is_str = is_short_str(av) || (is_cell(av) && (*as_pointer(av)).tag() == Tag::Str);
            if !is_str {
                if is_cell(av) {
                    __torajs_value_drop_heap(as_void_ptr(av));
                }
                if !first.is_null() {
                    __torajs_str_drop(first as *mut c_void);
                }
                __torajs_throw_type_error(c"locales must be strings or objects".as_ptr());
                return s as *mut u8;
            }
            let lc = crate::nanbox_ffi::__torajs_anyv_to_str(av) as *mut u8;
            if is_cell(av) {
                __torajs_value_drop_heap(as_void_ptr(av));
            }
            if __torajs_str_locale_check(lc) == 0 {
                __torajs_str_drop(lc as *mut c_void);
                if !first.is_null() {
                    __torajs_str_drop(first as *mut c_void);
                }
                return s as *mut u8;
            }
            if first.is_null() {
                first = lc;
            } else {
                __torajs_str_drop(lc as *mut c_void);
            }
        }
        let out = if upper != 0 {
            __torajs_str_to_locale_upper(s, first as *const u8)
        } else {
            __torajs_str_to_locale_lower(s, first as *const u8)
        };
        if !first.is_null() {
            __torajs_str_drop(first as *mut c_void);
        }
        out
    }
}
