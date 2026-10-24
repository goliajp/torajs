//! The module body's AsyncIteratorClose (§7.4.11) of `for await (v of
//! <any>)`, its awaits included. The body is the root of every job's
//! stack, so the close runs the jobs ahead of each await in place
//! (torajs-promise `module_await`), as the body's own awaits do.
//!
//! An iterator the `@@asyncIterator` GetIterator resolved has its
//! `return()` called and the answer awaited, which must then be an
//! object. Any other is a sync one under the Async-from-Sync wrapper
//! (§27.1.6.2.2 %AsyncFromSyncIteratorPrototype%.return), whose promise
//! the loop awaits: one job when there is no `return`, or when the
//! wrapper rejects it (a `return()` that throws or answers no object,
//! a throwing `done` / `value` read); else it is fulfilled from a
//! reaction on PromiseResolve of the answer's `value` (§27.1.6.4,
//! closeOnRejection false), one job after that settles, and a
//! rejection of it is raised then too.
//!
//! Under a throw completion every await still runs and its outcome is
//! dropped: the original throw wins (§7.4.11 step 6). The cancel throw
//! (an await that never settles) wins over both.

use core::ffi::c_void;

use crate::iter_any::USER_ASYNC_ITERATOR_LANE;
use crate::iter_any_close::reject_non_object_close_result;
use crate::iter_any_close_value::{ReturnCall, is_helper, iter_return};
use crate::iter_zip_shared::av_is_object;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_set(tag: i64, value: i64);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-microtask: one job, queued behind every job pending now
    fn __torajs_microtask_yield() -> i64;
    /// torajs-promise `module_await`
    fn __torajs_module_await_promise(src: *mut c_void, mask: u32, want: u32) -> i64;
    fn __torajs_module_await_slow(src: *mut c_void, code: i64, fused: i64, want_repr: i64) -> i64;
    fn __torajs_module_yield_rethrow();
    fn __torajs_module_cancelled() -> i64;
}

/// The close of the loop over `recv` whose cursor and iterator are in
/// `idx_slot` / `iter_slot` (the slots stay the caller's); `abrupt`:
/// the loop is left by a throw, which is pending.
///
/// # Safety
/// The caller is the module body; `recv` and `*iter_slot` are live
/// AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_close_defer(
    recv: AnyValue,
    idx_slot: *const i64,
    iter_slot: *const AnyValue,
    abrupt: i64,
) {
    unsafe {
        let kept = (abrupt != 0).then(|| (__torajs_throw_take_tag(), __torajs_throw_take()));
        if *idx_slot == USER_ASYNC_ITERATOR_LANE {
            async_close(*iter_slot);
        } else if *iter_slot == VALUE_UNDEFINED && is_helper(recv) {
            // an IterHelper loop steps the helper itself
            wrapper_close(recv);
        } else {
            wrapper_close(*iter_slot);
        }
        if let Some((tag, value)) = kept {
            restore(tag, value);
        }
    }
}

/// §7.4.11 steps 4-7 over an async iterator: a throwing GetMethod or
/// call is the completion, with nothing awaited.
unsafe fn async_close(iter: AnyValue) {
    unsafe {
        let ReturnCall::Answered(result) = iter_return(iter) else {
            return;
        };
        if await_in_place(result) == Some(false) {
            __torajs_throw_type_error(c"iterator.return() did not return an object".as_ptr());
        }
    }
}

/// The await of the Async-from-Sync wrapper's `return` over `iter`.
unsafe fn wrapper_close(iter: AnyValue) {
    unsafe {
        let result = match iter_return(iter) {
            ReturnCall::Absent => {
                __torajs_microtask_yield();
                return;
            }
            ReturnCall::Threw => {
                __torajs_module_yield_rethrow();
                return;
            }
            ReturnCall::Answered(result) => result,
        };
        if !av_is_object(result) {
            reject_non_object_close_result(result);
            __torajs_module_yield_rethrow();
            return;
        }
        let value = result_value(result);
        __torajs_anyv_rc_dec(result);
        let Some(value) = value else {
            __torajs_module_yield_rethrow();
            return;
        };
        if await_in_place(value).is_none() && __torajs_module_cancelled() != 0 {
            return;
        }
        if __torajs_throw_check() != 0 {
            __torajs_module_yield_rethrow();
        } else {
            __torajs_microtask_yield();
        }
    }
}

/// §27.1.6.4 steps 1-4 over the object `result` (borrowed): its `done`,
/// then its `value`, owned; `None` with a throw in flight.
unsafe fn result_value(result: AnyValue) -> Option<AnyValue> {
    use crate::iter_any_result::iter_result_get;
    unsafe {
        let ptr = as_void_ptr(result);
        __torajs_anyv_rc_dec(iter_result_get(result, ptr, b"done")?);
        iter_result_get(result, ptr, b"value")
    }
}

/// Await the owned `v` in place: `Some(is_object)` of its fulfillment,
/// `None` with the throw in flight (its rejection, a throwing `then`
/// read, the cancel throw).
unsafe fn await_in_place(v: AnyValue) -> Option<bool> {
    use crate::method_call_promise::{REPR_ANY, STATE_FULFILLED};
    unsafe {
        let cell = crate::promise_thenable::__torajs_anyv_promise_resolve(v as i64);
        let code = __torajs_module_await_promise(cell, 0xFF, STATE_FULFILLED as u32);
        // the value word in the any form, borrowed from the cell
        let raw = __torajs_module_await_slow(cell, code, 0, REPR_ANY as i64);
        let settled = (__torajs_throw_check() == 0).then(|| av_is_object(raw as AnyValue));
        __torajs_anyv_rc_dec(cell as AnyValue);
        settled
    }
}

/// Put the original throw back over whatever the close raised, unless
/// that is the cancel throw.
unsafe fn restore(tag: i64, value: i64) {
    unsafe {
        if __torajs_module_cancelled() != 0 {
            __torajs_anyv_rc_dec(crate::__torajs_anyv_box_from_pair(tag, value));
            return;
        }
        if __torajs_throw_check() != 0 {
            let raised = crate::__torajs_anyv_box_from_pair(
                __torajs_throw_take_tag(),
                __torajs_throw_take(),
            );
            __torajs_anyv_rc_dec(raised);
        }
        __torajs_throw_set(tag, value);
    }
}
