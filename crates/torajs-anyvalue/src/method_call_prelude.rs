//! The dispatch prelude — mids answered BEFORE
//! `method_call`'s shared nullish guard. Split to its own file when
//! the isPrototypeOf ordering arm took the dispatcher's host past the
//! 500-line cap.

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
    if mid == torajs_rc::ANY_METHOD_IS_PROTOTYPE_OF {
        return Some(unsafe { crate::method_call_object_proto::is_prototype_of(recv, argv, argc) });
    }
    None
}
