//! `BigInt.prototype` methods on an `any`-typed receiver (§21.2.3) —
//! the any-lane sibling reached from [`crate::method_call_cell`]'s
//! `Tag::BigInt` arm. `valueOf` (thisBigIntValue identity) is answered
//! by the cell-wide arm in `cell_method`; this arm owns the two
//! stringifiers:
//!
//! - `toString()` / `toString(radix)` — §6.1.6.2.13. A missing or
//!   explicit-`undefined` radix folds to base 10; the radix kernel
//!   records a pending RangeError for an out-of-`[2,36]` radix (and
//!   returns a throwaway empty Str), propagated at the extern
//!   boundary.
//! - `toLocaleString()` — grouped-decimal default-locale form.
//!
//! Both stringifiers return an owned Str cell (rc=1), boxed as an
//! AnyValue heap pointer.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_ffi::__torajs_anyv_to_number;

unsafe extern "C" {
    /// torajs-bigint §21.2.3 — decimal / radix / grouped-decimal
    /// stringify (owned Str cell out, rc=1); the radix form records a
    /// pending RangeError for an out-of-`[2,36]` radix and returns a
    /// throwaway empty Str, propagated at the extern boundary.
    fn __torajs_bigint_to_string(p: *const c_void) -> *mut u8;
    fn __torajs_bigint_to_string_radix(p: *const c_void, radix: i64) -> *mut u8;
    fn __torajs_bigint_to_locale_string(p: *const c_void) -> *mut u8;
}

/// Dispatch a BigInt-receiver method. `ptr` is the BigInt cell; a
/// name the arm doesn't own returns the no-such sentinel.
///
/// # Safety
/// `ptr` must be a valid BigInt heap cell; `argv`/`argc` describe the
/// call arguments.
pub(crate) unsafe fn bigint_method(
    ptr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if mid == ANY_METHOD_TO_STRING {
            let s = if argc >= 1 && *argv != VALUE_UNDEFINED {
                let radix = __torajs_anyv_to_number(*argv) as i64;
                __torajs_bigint_to_string_radix(ptr, radix)
            } else {
                __torajs_bigint_to_string(ptr)
            };
            return crate::nanbox_encode::__torajs_anyv_box_pointer(s as *mut c_void);
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING {
            let s = __torajs_bigint_to_locale_string(ptr);
            return crate::nanbox_encode::__torajs_anyv_box_pointer(s as *mut c_void);
        }
        crate::method_call::method_no_such()
    }
}
