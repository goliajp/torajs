//! `super.m(…)` / `super[k](…)` — the §13.3.6 call off a Super
//! Reference.
//!
//! A SuperProperty call is not the same thing as reading the method
//! and calling it. §13.3.7.3 MakeSuperPropertyReference builds a
//! reference whose BASE is the super base (GetSuperBase) but whose
//! `thisValue` is the CURRENT `this`; §13.3.6 EvaluateCall then
//! invokes with that `thisValue` as the receiver. Rewriting the site
//! as a plain member read off the base would hand the base itself to
//! the callee — right answer for the common `super.toString()`, wrong
//! for anything that reads `this`, and silently so.
//!
//! Four operands, all `any`: the super base, the property key (a
//! static string for `super.m`, a runtime value for `super[k]`), the
//! receiver, and one dense pack of the call's arguments — the same
//! packed-array protocol [`crate::super_call`] uses, which is what
//! lets one kernel take both spellings without a spread protocol.
//!
//! Order matters and is the spec's: RequireObjectCoercible on the
//! base (§13.3.7.3 step 4) BEFORE ToPropertyKey, so
//! `Object.setPrototypeOf(C, null); C.m()` raises on the base rather
//! than evaluating the key first.

use core::ffi::c_char;

use crate::method_call_closure::{apply_list, call_target};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
}

/// See module doc. Answers the call product; every gate records a
/// pending throw and rides `undefined` back, which the caller's
/// throw-check ends the path on.
///
/// # Safety
/// Cell operands are valid heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_super_prop_call(
    base: AnyValue,
    key: AnyValue,
    this_arg: AnyValue,
    list: AnyValue,
) -> AnyValue {
    unsafe {
        // §13.3.7.3 step 4 — RequireObjectCoercible(baseValue).
        if is_undefined(base) || is_null(base) {
            __torajs_throw_type_error(
                c"Cannot read properties of null or undefined (super property)".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §7.1.19 ToPropertyKey — a symbol key stays a symbol; NULL
        // means ToString threw on the key expression.
        let kp = crate::index_any_keyed::__torajs_anyv_to_property_key(key);
        if kp.is_null() {
            return VALUE_UNDEFINED;
        }
        let f =
            crate::member_get_receiver::__torajs_any_member_get_with_receiver(base, kp, this_arg);
        crate::index_any_keyed::__torajs_anyv_property_key_drop(kp);
        // A getter on the base can throw; its answer is undefined and
        // must not be mistaken for "not a function".
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        // §13.3.6 step 4 — IsCallable(func).
        let target = if is_cell(f) {
            call_target(as_void_ptr(f))
        } else {
            None
        };
        let Some(target) = target else {
            __torajs_anyv_rc_dec(f);
            __torajs_throw_type_error(c"super property is not a function".as_ptr());
            return VALUE_UNDEFINED;
        };
        let out = apply_list(&target, this_arg, list);
        __torajs_anyv_rc_dec(f);
        out
    }
}
