//! The result of an async function whose body never awaits. The body
//! runs to its end inside the call (§27.7.5.2 AsyncBlockStart: nothing
//! suspends it), so it needs no frame: its return makes the result
//! promise and settles it in one call. The promise exists before the
//! body in the specification (§15.8.4 EvaluateAsyncFunctionBody step
//! 1), but nothing can observe it until the call returns it, so making
//! it at the return is the same program.

use core::ffi::c_void;

use super::body::settle_result;
use crate::layout::{REPR_ANY, as_promise};
use crate::state::__torajs_promise_reject;
use crate::then_box::__torajs_anyv_box_from_pair;

/// The result of a body that returned `value`, in the form `repr`
/// names: fulfilled with it, or, for a promise or another thenable,
/// adopting it (§27.2.1.3.2). The value's reference goes to the
/// result; the caller gets the result's only reference.
///
/// # Safety
/// `value` is owned and in the form `repr` names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_async_resolved(repr: i64, value: i64) -> *mut c_void {
    unsafe {
        let result = super::__torajs_frame_result_alloc(repr);
        // nobody has seen the result, so its list is empty
        settle_result(result, repr as u8, value);
        result
    }
}

/// The result of a body that raised: rejected with the throw, which
/// the body took off the throw TLS as its (tag, value) pair. The body
/// takes it itself, as a catch does, so that nothing reads the throw
/// as still pending once the call has returned.
///
/// # Safety
/// `(tag, value)` is a taken throw; its stake goes to the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_async_rejected(tag: i64, value: i64) -> *mut c_void {
    unsafe {
        let result = super::__torajs_frame_result_alloc(REPR_ANY as i64);
        reject_with_taken(result, tag, value);
        result
    }
}

/// Reject `result` with a throw already taken off the throw TLS as its
/// (tag, value) pair: the pair is boxed and the cell stamped `REPR_ANY`,
/// so a handler converts the reason into the lane it declared, and the
/// box holds the throw's stake.
///
/// # Safety
/// `result` is a live pending promise; the pair is a taken throw, whose
/// stake goes to the box.
pub(crate) unsafe fn reject_with_taken(result: *mut c_void, tag: i64, value: i64) {
    unsafe {
        let reason = __torajs_anyv_box_from_pair(tag, value);
        let pp = as_promise(result);
        (*pp).value_is_heap = 1;
        (*pp).value_repr = REPR_ANY;
        __torajs_promise_reject(result, reason as i64);
    }
}
