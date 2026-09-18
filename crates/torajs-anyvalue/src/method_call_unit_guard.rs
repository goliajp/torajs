//! The call-boundary brand guard of a per-evaluation class (RFC
//! 20260917-class-eval-unit K2b / K4-4): whether the evaluation that
//! minted a method face admits the receiver a rebind hands it.
//!
//! §15.7.14 gives every evaluation of a unit class its own method
//! function objects, and §7.3.31 binds a private element to the
//! evaluation that declared it. Two evaluations share a tag, a layout
//! and every private key spelling, so a body run against the OTHER
//! evaluation's receiver finds each element it asks for: legal for a
//! public member (`C1.prototype.plain.call(c2)` answers, as bun does),
//! a brand failure the moment the body reaches a private one. The face
//! knows which evaluation minted it; an instance carries its own in a
//! hidden slot, and a static member's receiver IS the class object.
//!
//! The throw lands at the call boundary rather than at the private
//! access inside, so a side effect the body would have run first does
//! not (registered deviation).

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-meta — the class object a unit-class instance carries
    /// in its hidden slot (0 = not one).
    fn __torajs_class_unit_of(cell: *const c_void) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Record the brand TypeError and answer `true` when `face` was minted
/// by one evaluation, its body reaches a private element, and
/// `this_arg` belongs to another: neither that evaluation's class
/// object (a static member's receiver) nor one of its instances. A
/// receiver of the right evaluation but the wrong kind — an instance
/// handed to a static body — passes here and fails the body's own
/// private access, which finds no such element on it.
///
/// # Safety
/// `face` is a live `Tag::Closure` cell; `this_arg` a live AnyValue.
pub(crate) unsafe fn refuses_receiver(face: *mut c_void, this_arg: AnyValue) -> bool {
    unsafe {
        let Some((face_unit, true)) = crate::method_value_class::class_method_face_unit(face)
        else {
            return false;
        };
        if is_cell(this_arg) {
            let recv = as_void_ptr(this_arg);
            if recv as u64 == face_unit || __torajs_class_unit_of(recv) == face_unit {
                return false;
            }
        }
        __torajs_throw_type_error(
            c"cannot access a private member of another evaluation of this class".as_ptr(),
        );
        true
    }
}

/// Run a STATIC face (`(tag 0, twin ≠ 0)`) with `this_arg` as its
/// receiver: the twin takes it recv-first, after the guard above.
///
/// # Safety
/// `face` is a live class-method face cell whose `twin` is a live
/// boxed-adapter address; `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn invoke_static_face(
    face: *mut c_void,
    twin: u64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if refuses_receiver(face, this_arg) {
            return VALUE_UNDEFINED;
        }
        crate::method_call_closure_dispatch::invoke_boxed_recv_first(
            face, twin, this_arg, argv, argc,
        )
    }
}
