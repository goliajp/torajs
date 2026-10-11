//! Numeric-immediate arm of `__torajs_any_method_call`
//! (Any-method-call RFC 20260704 C4-3b) — `n.toFixed(2)` /
//! `i.toString(16)` where the number crossed into the `any` world.
//!
//! The receiver is a NaN-box immediate (int32 or double), not a
//! cell, so this arm sits before the cell dispatch. It mirrors the
//! typed tier's numeric method path
//! (`ssa_lower_call_number_methods.rs`) onto the same
//! `__torajs_num_*` / `__torajs_{i64,f64}_to_str` kernels, picking
//! the i / f kernel variant off the box encoding (int32 → i, double
//! → f — exactly the typed tier's I64/F64 split):
//!
//! - `toString()` bare / explicit-undefined radix → the shared
//!   Number-to-String formatters; `toString(radix)` → the radix
//!   kernels (RangeError outside [2,36] rides TLS; the call site's
//!   SSA throw check propagates).
//! - `toFixed(d)` — missing/undefined digits default to 0 (ES
//!   §21.1.3.3); `toExponential(d)` — missing digits ride the
//!   typed tier's i64::MIN "shortest" sentinel; `toPrecision(p)` —
//!   missing precision routes through bare ToString (§21.1.3.5
//!   step 2). All three RangeError through TLS.
//! - `toLocaleString()` — locale/options args ignored (tr subset is
//!   always en-US, S139).
//! - `valueOf()` — identity: the immediate itself is the boxed
//!   return.
//!
//! Digits/radix arguments decode through `anyv_to_number` and
//! truncate to i64 (the typed tier's FpToSi shape).

use torajs_rc::{
    ANY_METHOD_TO_EXPONENTIAL, ANY_METHOD_TO_FIXED, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_PRECISION, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use crate::method_call::method_no_such;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_double};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_to_number;
use crate::{__torajs_f64_to_str, __torajs_i64_to_str};

unsafe extern "C" {
    fn __torajs_num_to_string_radix_i(v: i64, radix: i64) -> *mut u8;
    fn __torajs_num_to_string_radix_f(v: f64, radix: i64) -> *mut u8;
    fn __torajs_num_to_fixed_i(v: i64, digits: i64) -> *mut u8;
    fn __torajs_num_to_fixed_f(v: f64, digits: i64) -> *mut u8;
    fn __torajs_num_to_exp_i(v: i64, digits: i64) -> *mut u8;
    fn __torajs_num_to_exp_f(v: f64, digits: i64) -> *mut u8;
    fn __torajs_num_to_precision_i(v: i64, p: i64) -> *mut u8;
    fn __torajs_num_to_precision_f(v: f64, p: i64) -> *mut u8;
    fn __torajs_num_to_locale_i(v: i64) -> *mut u8;
    fn __torajs_num_to_locale_f(v: f64) -> *mut u8;
}

/// Typed-tier "shortest exponential" sentinel for a no-arg
/// `toExponential()` (mirror of the ConstI64(i64::MIN) the lowerer
/// passes there).
const EXP_SHORTEST: i64 = i64::MIN;

/// Numeric-immediate arm — see module doc.
pub(crate) unsafe fn number_method(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let is_f = is_double(recv);
    let n = unsafe { __torajs_anyv_to_number(recv) };
    // First-argument decode: `None` = missing or explicit undefined
    // (both fold to the method's default per ES §21.1.3.{3,5,6}).
    // Lazy — a no-such-method miss (e.g. `Date.prototype.setDate
    // .call(0, arg)` landing here off the int32 receiver) must
    // throw its TypeError WITHOUT coercing arguments first
    // ("validation precedes input coercion", §21.4.4.20 step order).
    let arg0 = || -> Option<i64> {
        if argc >= 1 {
            let av = unsafe { *argv.add(0) };
            if av == VALUE_UNDEFINED {
                None
            } else {
                Some(unsafe { __torajs_anyv_to_number(av) as i64 })
            }
        } else {
            None
        }
    };
    let boxed = |p: *mut u8| unsafe { __torajs_anyv_box_from_pair(4, p as i64) };
    unsafe {
        match mid {
            m if m == ANY_METHOD_VALUE_OF => recv,
            m if m == ANY_METHOD_TO_STRING => match arg0() {
                Some(radix) if is_f => boxed(__torajs_num_to_string_radix_f(n, radix)),
                Some(radix) => boxed(__torajs_num_to_string_radix_i(n as i64, radix)),
                None if is_f => boxed(__torajs_f64_to_str(n) as *mut u8),
                None => boxed(__torajs_i64_to_str(n as i64) as *mut u8),
            },
            m if m == ANY_METHOD_TO_FIXED => {
                let d = arg0().unwrap_or(0);
                if is_f {
                    boxed(__torajs_num_to_fixed_f(n, d))
                } else {
                    boxed(__torajs_num_to_fixed_i(n as i64, d))
                }
            }
            m if m == ANY_METHOD_TO_EXPONENTIAL => {
                let d = arg0().unwrap_or(EXP_SHORTEST);
                if is_f {
                    boxed(__torajs_num_to_exp_f(n, d))
                } else {
                    boxed(__torajs_num_to_exp_i(n as i64, d))
                }
            }
            m if m == ANY_METHOD_TO_PRECISION => match arg0() {
                Some(p) if is_f => boxed(__torajs_num_to_precision_f(n, p)),
                Some(p) => boxed(__torajs_num_to_precision_i(n as i64, p)),
                // §21.1.3.5 step 2 — no precision routes to ToString.
                None if is_f => boxed(__torajs_f64_to_str(n) as *mut u8),
                None => boxed(__torajs_i64_to_str(n as i64) as *mut u8),
            },
            m if m == ANY_METHOD_TO_LOCALE_STRING => {
                if is_f {
                    boxed(__torajs_num_to_locale_f(n))
                } else {
                    boxed(__torajs_num_to_locale_i(n as i64))
                }
            }
            m if crate::method_call_arraylike::arraylike_supported(m)
                && !crate::method_call_arraylike_mut::arraylike_mut_supported(m) =>
            {
                // §23.1.3 step 1 — ToObject(num) mints the wrapper;
                // its own-miss reads walk to the Number.prototype
                // expando face (RFC 20260721 G2b) and the callback's
                // O answers `obj instanceof Number`.
                crate::method_call_arraylike::arraylike_on_minted_wrapper(recv, m, argv, argc)
            }
            m if crate::method_call_arraylike_mut::arraylike_mut_supported(m) => {
                match crate::method_call_arraylike_mut_prim::prim_mut_method(0, recv, m, argv, argc)
                {
                    Some(out) => out,
                    None => method_no_such(),
                }
            }
            _ => method_no_such(),
        }
    }
}
