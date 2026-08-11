//! §21.4.2.1 `new Date(value)` — the one-argument constructor over a
//! runtime value.
//!
//! `__torajs_date_from_ms` covers the number fast lane (a `Number`
//! literal keeps that route in the desugar), but the spec's step 4
//! is richer: a Date argument copies its [[DateValue]] directly
//! (no ToPrimitive — a user `valueOf` on a Date instance must NOT
//! run), any other object goes through ToPrimitive with NO hint
//! (`"default"` reaches a user `@@toPrimitive`, the
//! value-symbol-to-prim t262 family), and a String primitive —
//! whether written literally or answered by a hook — PARSES rather
//! than ToNumbers.

use core::ffi::c_void;

use torajs_rc::{HeapHeader, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_number, __torajs_anyv_to_str};
use crate::to_primitive::{heap_to_primitive_default, is_object_value};

unsafe extern "C" {
    fn __torajs_date_from_ms(ms: f64) -> *mut c_void;
    fn __torajs_date_from_iso(str_ptr: *const c_void) -> *mut c_void;
    fn __torajs_date_get_time(d_ptr: *const c_void) -> f64;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
}

/// A type-correct placeholder when a coercion left a throw in
/// flight — an invalid date the caller's `emit_throw_check` unwinds
/// past without ever observing.
unsafe fn invalid_date() -> *mut c_void {
    unsafe { __torajs_date_from_ms(f64::NAN) }
}

/// §21.4.2.1 step 4.b.ii-iv over an already-primitive value: a
/// String parses (§21.4.3.2 order), everything else ToNumbers into
/// TimeClip. Borrows `prim`.
unsafe fn date_from_primitive(prim: AnyValue) -> *mut c_void {
    unsafe {
        let is_str = is_short_str(prim)
            || (is_cell(prim) && (*(as_void_ptr(prim) as *const HeapHeader)).tag() == Tag::Str);
        if is_str {
            // to_str is identity-shaped for a string primitive and
            // normalizes all three reprs (ShortStr / Str / Substr
            // view) into the one cell the parser reads.
            let s = __torajs_anyv_to_str(prim);
            let d = __torajs_date_from_iso(s);
            __torajs_str_drop(s);
            return d;
        }
        let n = __torajs_anyv_to_number(prim);
        if __torajs_throw_check() != 0 {
            // ToNumber refused (a Symbol / BigInt argument).
            return invalid_date();
        }
        __torajs_date_from_ms(n)
    }
}

/// `new Date(value)` — §21.4.2.1 step 4 (numberOfArgs == 1).
///
/// # Safety
/// `v` is a live AnyValue; cross-tier extern kernels must be
/// linkable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_from_value(v: AnyValue) -> *mut c_void {
    unsafe {
        if is_cell(v) {
            let ptr = as_void_ptr(v) as *mut c_void;
            let h = &*(ptr as *const HeapHeader);
            // step 4.a — [[DateValue]] is copied without observable
            // method calls.
            if h.tag() == Tag::Date {
                return __torajs_date_from_ms(__torajs_date_get_time(ptr));
            }
            // step 4.b — ToPrimitive(value), no hint: a user
            // @@toPrimitive receives "default".
            if is_object_value(v) {
                let Some(prim) = heap_to_primitive_default(ptr) else {
                    // Both coercion methods refused — TypeError
                    // recorded.
                    return invalid_date();
                };
                if __torajs_throw_check() != 0 {
                    __torajs_anyv_rc_dec(prim);
                    return invalid_date();
                }
                let d = date_from_primitive(prim);
                __torajs_anyv_rc_dec(prim);
                return d;
            }
        }
        date_from_primitive(v)
    }
}
