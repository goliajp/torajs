//! The dispatch prelude — the mids the dispatcher answers around its
//! shared nullish guard, before the per-tag ladder gets a say. Split
//! to its own file when the isPrototypeOf ordering arm took the
//! dispatcher's host past the 500-line cap, and the post-guard half
//! joined it when the same host's fn hit the 200-line one.
//!
//! Two arms, and the guard is what separates them:
//! [`pre_nullish_arm`] holds the mids that have their own answer for
//! a null / undefined receiver, [`post_nullish_arm`] the ones whose
//! step 1 is ToObject(this) and so must let the guard throw first.

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The mids that dispatch BEFORE the shared nullish guard — each has
/// its own answer for a null / undefined receiver, so the guard must
/// not see them: §20.1.3.6's badge classifier answers on EVERY
/// this-value (steps 1-2, no ToObject throw; reached only through the
/// reified badge cell — a plain `toString` name never interns to this
/// mid), §10.2.4 %ThrowTypeError% raises ITS message whatever the
/// receiver, and §20.1.3.3 orders step 1's primitive-V `false` before
/// step 2's ToObject(this) can throw (so `isPrototypeOf.call(null,
/// 5)` is `false`, not a TypeError — the kernel runs both steps in
/// that order).
pub(crate) unsafe fn pre_nullish_arm(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> Option<AnyValue> {
    if mid == torajs_rc::ANY_METHOD_OBJECT_TO_STRING {
        return Some(unsafe { crate::method_call_object_proto::object_proto_to_string(recv) });
    }
    if mid == torajs_rc::ANY_METHOD_THROW_TYPE_ERROR {
        unsafe {
            __torajs_throw_type_error(
                c"'caller', 'callee', and 'arguments' properties may not be accessed".as_ptr(),
            );
        }
        return Some(VALUE_UNDEFINED);
    }
    // §20.1.3.3 is an ORDINARY %Object.prototype% method, so an own
    // / class / chain / patched `isPrototypeOf` has to win over it —
    // it is answered at the end of the walk with the other two
    // (`object_proto_universal`). Only a reified cell's re-dispatch
    // takes it here, because that IS the body running, and it is
    // pre-nullish for the reason above: `Object.prototype
    // .isPrototypeOf.call(null, 5)` is step 1's `false`, not step 2's
    // ToObject throw.
    if mid == torajs_rc::ANY_METHOD_IS_PROTOTYPE_OF && skip_wrapper_expando {
        return Some(unsafe { crate::method_call_object_proto::is_prototype_of(recv, argv, argc) });
    }
    None
}

/// The mids answered AFTER the shared nullish guard and before the
/// per-tag ladder. All three are reified-cell bodies rather than
/// name lookups, so `skip_wrapper_expando` gates the two that a
/// plain-named call must not reach.
///
/// The %Object.prototype% three (§20.1.4.3 / §20.1.4.5 / §20.1.3.3 —
/// see [`crate::method_call_object_proto::object_proto_universal`])
/// are answered at the END of the walk so an own / class / chain /
/// patched one wins; only the re-dispatch takes them here, because
/// that IS the body running. Their place is after the guard for a
/// reason worth keeping next to
/// [`pre_nullish_arm`]'s: §20.1.4.3 step 1 is ToObject(this) with
/// nothing ordered before it, so `hasOwnProperty.call(null, "x")`
/// throws where §20.1.3.3's `isPrototypeOf.call(null, 5)` answers
/// `false`.
///
/// # Safety
/// Same contract as the dispatcher: `argv` holds `argc` live slots.
pub(crate) unsafe fn post_nullish_arm(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> Option<AnyValue> {
    // §23.1.3.36 — the reified `Array.prototype.toString` cell
    // borrowed across receivers (RFC 20260721 刀 11 G12): join for an
    // Array, `Get(this, "join")` else, badge fallback.
    if mid == torajs_rc::ANY_METHOD_ARR_TO_STRING {
        return Some(unsafe { crate::method_call_object_proto::arr_to_string_borrowed(recv) });
    }
    if !skip_wrapper_expando {
        return None;
    }
    // §21.4.4.37 — the reified `Date.prototype.toJSON` cell's
    // [[Call]] body is receiver-generic (ToPrimitive number
    // non-finite → null, else Invoke toISOString). Redispatch-only:
    // a plain-named `obj.toJSON()` keeps ordinary own-property
    // routing (a user object's own `toJSON` must win).
    if mid == torajs_rc::ANY_METHOD_TO_JSON {
        return Some(unsafe { crate::method_call_date::date_to_json_generic(recv) });
    }
    unsafe { crate::method_call_object_proto::object_proto_universal(recv, mid, argv, argc) }
}
