//! `super[k]` / `super[k] = v` / `super[k](…)` — reads, writes and
//! calls off a Super Reference.
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
//! The write is the third face of the same reference: §9.1.9
//! OrdinarySet runs against the BASE's chain but stores onto the
//! RECEIVER, which is why `super.x = v` on a plain data property
//! creates an OWN property of `this` while an inherited setter on the
//! base still runs.
//!
//! The read alone has the same problem in a quieter form: a getter
//! on the base runs against whoever the reference names as
//! `thisValue`, and a plain member read off the base names the BASE.
//! `super[k]` therefore travels the same kernel minus the invoke.
//!
//! Order matters and is the spec's: RequireObjectCoercible on the
//! base (§13.3.7.3 step 4) BEFORE ToPropertyKey, so
//! `Object.setPrototypeOf(C, null); C.m()` raises on the base rather
//! than evaluating the key first.

use core::ffi::c_char;

use core::ffi::c_void;

use crate::method_call_closure::{apply_list, call_target};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
}

/// §13.3.7.3 steps 4-5 plus the receiver-aware [[Get]] — the half
/// both spellings share. `None` means a gate recorded a pending
/// throw and the caller must ride `undefined` back.
///
/// # Safety
/// Cell operands are valid heap pointers.
unsafe fn super_get(base: AnyValue, key: AnyValue, this_arg: AnyValue) -> Option<AnyValue> {
    unsafe {
        // §13.3.7.3 step 4 — RequireObjectCoercible(baseValue).
        if is_undefined(base) || is_null(base) {
            __torajs_throw_type_error(
                c"Cannot read properties of null or undefined (super property)".as_ptr(),
            );
            return None;
        }
        // §7.1.19 ToPropertyKey — a symbol key stays a symbol; NULL
        // means ToString threw on the key expression.
        let kp = crate::index_any_keyed::__torajs_anyv_to_property_key(key);
        if kp.is_null() {
            return None;
        }
        let v =
            crate::member_get_receiver::__torajs_any_member_get_with_receiver(base, kp, this_arg);
        crate::index_any_keyed::__torajs_anyv_property_key_drop(kp);
        // A getter on the base can throw; its answer is undefined and
        // must not be mistaken for the property's value.
        if __torajs_throw_check() != 0 {
            return None;
        }
        Some(v)
    }
}

/// `super[k]` — the read, carrying §13.3.7's receiver into any
/// accessor the base declares. The answer is OWNED, as every member
/// read's is.
///
/// # Safety
/// Cell operands are valid heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_super_prop_get(
    base: AnyValue,
    key: AnyValue,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe { super_get(base, key, this_arg).unwrap_or(VALUE_UNDEFINED) }
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
        let Some(f) = super_get(base, key, this_arg) else {
            return VALUE_UNDEFINED;
        };
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

/// `super[k] = v` — §9.1.9 OrdinarySet with the receiver §13.3.7
/// names: the base's chain decides whether a setter runs, the
/// receiver is what a data write lands on. Answers the assignment's
/// value, which is what an assignment expression evaluates to.
///
/// A refused write raises §13.15.2's strict-assignment TypeError.
/// Recorded boundary: tr has no per-function sloppy write flavor for
/// this lane, so the sloppy silent-no-op spelling is not available
/// here — the strict answer is the one every other member write in
/// the language takes.
///
/// # Safety
/// Cell operands are valid heap pointers; `value` is an owned box.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_super_prop_set(
    base: AnyValue,
    key: AnyValue,
    value: AnyValue,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        if is_undefined(base) || is_null(base) {
            __torajs_throw_type_error(
                c"Cannot set properties of null or undefined (super property)".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let kp = crate::index_any_keyed::__torajs_anyv_to_property_key(key);
        if kp.is_null() {
            return VALUE_UNDEFINED;
        }
        // The owned unbox mints the +1 the entry write consumes.
        let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(value);
        let payload = crate::nanbox_encode::__torajs_anyv_unbox_value_owned(value);
        // The receiver rides a local slot: a dynobj resize swaps the
        // cell inside the kernel and the caller's box is not written
        // back (the `reflect_set` posture).
        let mut recv = this_arg;
        let wrote = crate::member_set_receiver::__torajs_any_member_set_with_receiver(
            base,
            kp as *mut c_void,
            tag as u64,
            payload as u64,
            &mut recv as *mut AnyValue,
        );
        crate::index_any_keyed::__torajs_anyv_property_key_drop(kp);
        if wrote == 0 && __torajs_throw_check() == 0 {
            __torajs_throw_type_error(c"cannot assign to a super property".as_ptr());
        }
        // An assignment expression answers the value it stored, and
        // this answer is OWNED like every other kernel's — the store
        // took its own share above, so the answer needs one too.
        crate::nanbox_ffi::__torajs_anyv_rc_inc(value);
        value
    }
}
