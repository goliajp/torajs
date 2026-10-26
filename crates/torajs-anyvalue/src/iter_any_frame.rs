//! `for await (v of <any>)` in the body of an async function that
//! suspends over a frame. The body runs inside a job, so nothing here
//! runs the job queue: each step and the close answer a code naming
//! what the body awaits or how many jobs it lets run before going on,
//! and the body suspends there (torajs-core `ssa_lower_fn::suspend`).
//! The module body's twins (`__torajs_any_iter_next_defer`,
//! `__torajs_any_iter_close_defer`) await the same points in place.

use crate::iter_any::{OPEN_THREW_LANE, USER_ASYNC_ITERATOR_LANE};
use crate::iter_any_await::AwaitMode;
use crate::iter_any_close_value::{
    ReturnCall, WrapperReturn, is_helper, iter_return, wrapper_return,
};
use crate::iter_zip_shared::av_is_object;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// A step's throw the body raises one job later: a sync lane's step
/// that threw rejects the Async-from-Sync wrapper's promise
/// (§27.1.6.2.1 IfAbruptRejectPromise), whose await goes on a job
/// after.
pub const STEP_RAISE_LATER: i64 = 4;

/// The frame body's step: as `__torajs_any_iter_next_defer`, with the
/// one in-place job of a sync lane's throw answered as
/// [`STEP_RAISE_LATER`], the throw still in flight.
///
/// # Safety
/// As `__torajs_any_iter_next`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next_frame(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        let mode = AwaitMode::Defer;
        let code = crate::iter_any::iter_next_inner(recv, idx_slot, iter_slot, out, false, mode);
        let lane = *idx_slot;
        if code == 0
            && lane != USER_ASYNC_ITERATOR_LANE
            && lane != OPEN_THREW_LANE
            && __torajs_throw_check() != 0
        {
            return STEP_RAISE_LATER;
        }
        code
    }
}

/// The close is over; a throw it raised is in flight.
pub const CLOSE_DONE: i64 = 0;
/// One job, then the close is over.
pub const CLOSE_YIELD: i64 = 1;
/// A throw is in flight, raised one job later.
pub const CLOSE_YIELD_RAISE: i64 = 2;
/// `*out` holds what an async iterator's `return()` answered, owned:
/// it is awaited, a rejection raised at once, and the fulfillment must
/// be an object ([`__torajs_any_iter_close_result_check`]).
pub const CLOSE_AWAIT_RESULT: i64 = 3;
/// `*out` holds the `value` of what the Async-from-Sync wrapper's
/// `return` answered, owned: it is awaited (§27.1.6.4), and the close
/// goes on one job after it settles, a rejection raised then.
pub const CLOSE_AWAIT_VALUE: i64 = 4;

/// The frame body's AsyncIteratorClose (§7.4.11) of the loop over
/// `recv` whose cursor and iterator are in `idx_slot` / `iter_slot`,
/// up to its first await: answers one of the `CLOSE_*` codes. Run with
/// no throw in flight; under a throw completion the body sets the
/// original aside first and discards whatever the close raises.
///
/// # Safety
/// `recv` and `*iter_slot` are live AnyValues; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_close_frame(
    recv: AnyValue,
    idx_slot: *const i64,
    iter_slot: *const AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        *out = VALUE_UNDEFINED;
        if *idx_slot == USER_ASYNC_ITERATOR_LANE {
            return match iter_return(*iter_slot) {
                ReturnCall::Answered(result) => {
                    *out = result;
                    CLOSE_AWAIT_RESULT
                }
                ReturnCall::Absent | ReturnCall::Threw => CLOSE_DONE,
            };
        }
        // an IterHelper loop steps the helper itself
        let iter = if *iter_slot == VALUE_UNDEFINED && is_helper(recv) {
            recv
        } else {
            *iter_slot
        };
        wrapper_close(iter, out)
    }
}

/// §27.1.6.2.2 %AsyncFromSyncIteratorPrototype%.return over the sync
/// iterator `iter`, up to the await of its answer's `value`.
unsafe fn wrapper_close(iter: AnyValue, out: *mut AnyValue) -> i64 {
    unsafe {
        match wrapper_return(iter) {
            WrapperReturn::Absent => CLOSE_YIELD,
            WrapperReturn::Rejected => CLOSE_YIELD_RAISE,
            WrapperReturn::Value(value) => {
                *out = value;
                CLOSE_AWAIT_VALUE
            }
        }
    }
}

/// §7.4.11 step 9: the awaited answer of an async iterator's `return()`
/// (`v`, borrowed) must be an object.
///
/// # Safety
/// `v` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_close_result_check(v: AnyValue) {
    unsafe {
        if !av_is_object(v) {
            __torajs_throw_type_error(c"iterator.return() did not return an object".as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter_any_await::{STEP_AWAIT_RESULT, STEP_AWAIT_VALUE};

    /// torajs-core spells these codes in the branches it lowers
    /// (`ssa_lower_fn::suspend::any_step` / `any_frame`); a change here
    /// is a change there.
    #[test]
    fn step_and_close_codes() {
        assert_eq!(
            [STEP_AWAIT_VALUE, STEP_AWAIT_RESULT, STEP_RAISE_LATER],
            [2, 3, 4]
        );
        assert_eq!(
            [
                CLOSE_DONE,
                CLOSE_YIELD,
                CLOSE_YIELD_RAISE,
                CLOSE_AWAIT_RESULT,
                CLOSE_AWAIT_VALUE
            ],
            [0, 1, 2, 3, 4]
        );
    }
}
