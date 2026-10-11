//! ES402 CanonicalizeLocaleList (§9.2.1) for `toLocale{Upper,Lower}Case`
//! over a locales argument that is not a plain string: an array, any
//! other object (array-like), `null`, a primitive. Every element is
//! validated (TypeError on a value that is neither a string nor an
//! object, RangeError on the first structurally invalid tag); an object
//! element is ToString'd; then the first tag selects the tailoring the
//! torajs-str kernel applies.
//!
//! Lives on the anyvalue side because walking the list requires the
//! array-like Get / HasProperty faces and AnyValue decoding; the per-tag
//! validation and the case fold itself are cross-TU calls into
//! torajs-str.

use core::ffi::c_void;

use crate::nanbox::{
    as_pointer, as_void_ptr, box_void_ptr, is_cell, is_null, is_short_str, is_undefined,
};
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-str — structural language-tag validation; records a
    /// pending RangeError and answers 0 on an invalid tag.
    fn __torajs_str_locale_check(locale: *const u8) -> i64;
    /// torajs-str — the tailored case-fold kernels.
    fn __torajs_str_to_locale_upper(s: *const u8, locale: *const u8) -> *mut u8;
    fn __torajs_str_to_locale_lower(s: *const u8, locale: *const u8) -> *mut u8;
    /// torajs-str — Str drop.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — pending-throw probe.
    fn __torajs_throw_check() -> i64;
    /// torajs-throw — catchable TypeError raise (TLS pending).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `s.toLocale{Upper,Lower}Case(localeArr)` with an Arr locales
/// argument — the array is an object, so it takes the general walk.
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
    unsafe { __torajs_str_locale_case_any(s, box_void_ptr(arr as *mut c_void), upper) }
}

/// `s.toLocale{Upper,Lower}Case(locales)` for any `locales` value
/// (§9.2.1): `undefined` → the host default; a string → that one tag;
/// `null` → TypeError (ToObject); a primitive wrapper has no elements →
/// the host default; an object is walked as an array-like, skipping
/// holes. On a pending throw the receiver is echoed as the stand-in
/// (the call site's throw check kills the path). `locales` is borrowed.
///
/// # Safety
///
/// `s` is a valid Str heap block; a cell payload of `locales` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_case_any(
    s: *const u8,
    locales: u64,
    upper: i64,
) -> *mut u8 {
    unsafe {
        if is_undefined(locales) {
            return fold(s, core::ptr::null(), upper);
        }
        if is_null(locales) {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object".as_ptr());
            return s as *mut u8;
        }
        if is_string(locales) {
            let lc = crate::nanbox_ffi::__torajs_anyv_to_str(locales) as *mut u8;
            let out = fold(s, lc, upper);
            __torajs_str_drop(lc as *mut c_void);
            return out;
        }
        if !is_object(locales) {
            // ToObject of a number / boolean / symbol / bigint: a
            // wrapper with no `length`, so no elements.
            return fold(s, core::ptr::null(), upper);
        }
        let Ok(first) = first_valid_tag(as_void_ptr(locales)) else {
            return s as *mut u8;
        };
        let out = fold(s, first, upper);
        if !first.is_null() {
            __torajs_str_drop(first as *mut c_void);
        }
        out
    }
}

/// The steps 7.a-g walk over an object `o`: the first tag (owned, NULL
/// for an empty list), or `Err` with a pending throw recorded.
///
/// # Safety
/// `o` is a live heap object.
pub(crate) unsafe fn first_valid_tag(o: *mut c_void) -> Result<*mut u8, ()> {
    unsafe {
        let len = crate::method_call_arraylike_host::arraylike_len(o).unwrap_or(0);
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        let mut first: *mut u8 = core::ptr::null_mut();
        let release = |first: *mut u8| {
            if !first.is_null() {
                __torajs_str_drop(first as *mut c_void);
            }
        };
        for k in 0..len {
            if !crate::method_call_arraylike_host::arraylike_has(o, k) {
                continue;
            }
            let v = crate::method_call_arraylike_host::arraylike_get(o, k);
            if __torajs_throw_check() != 0 {
                release(first);
                return Err(());
            }
            if !is_string(v) && !is_object(v) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                release(first);
                __torajs_throw_type_error(c"locales must be strings or objects".as_ptr());
                return Err(());
            }
            // Step 7.c.iv / v — a string is the tag; an object's
            // ToString (which may run its toString) is.
            let lc = crate::nanbox_ffi::__torajs_anyv_to_str(v) as *mut u8;
            crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            if __torajs_throw_check() != 0 || __torajs_str_locale_check(lc) == 0 {
                __torajs_str_drop(lc as *mut c_void);
                release(first);
                return Err(());
            }
            if first.is_null() {
                first = lc;
            } else {
                __torajs_str_drop(lc as *mut c_void);
            }
        }
        Ok(first)
    }
}

/// The tailored fold, `locale` NULL = the host default.
///
/// # Safety
/// `s` is a valid Str heap block; `locale` NULL or one too.
unsafe fn fold(s: *const u8, locale: *const u8, upper: i64) -> *mut u8 {
    unsafe {
        if upper != 0 {
            __torajs_str_to_locale_upper(s, locale)
        } else {
            __torajs_str_to_locale_lower(s, locale)
        }
    }
}

/// A string value: a short string or a heap Str cell.
pub(crate) fn is_string(v: u64) -> bool {
    is_short_str(v) || (is_cell(v) && unsafe { (*as_pointer(v)).tag() } == Tag::Str)
}

/// An ECMAScript Object: a heap cell that is not a string, symbol,
/// bigint or the undefined oddball.
pub(crate) fn is_object(v: u64) -> bool {
    is_cell(v)
        && !matches!(
            unsafe { (*as_pointer(v)).tag() },
            Tag::Str | Tag::Symbol | Tag::BigInt | Tag::Undefined
        )
}
