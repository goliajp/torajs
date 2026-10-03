//! The reaction job behind the `any` lane's `.then` / `.catch` /
//! `.finally`: the packed attach state and the dispatcher that runs
//! it once the source settles.

use core::ffi::c_void;

use super::{
    __torajs_promise_drop, __torajs_promise_reject, __torajs_promise_resolve, __torajs_throw_check,
    __torajs_throw_take, __torajs_throw_take_tag, __torajs_value_drop_heap, P_IS_HEAP_OFF,
    P_REPR_OFF, P_STATE_OFF, P_VALUE_OFF, REPR_ANY, STATE_REJECTED, box_settled, free,
    resolve_result_with, stamp_result,
};
use crate::method_call_closure_dispatch::invoke_boxed;
use crate::nanbox::VALUE_UNDEFINED;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use torajs_rc::__torajs_rc_inc;

/// Packed attach state for [`then_any_dispatch`]. Every closure and
/// `result` hold their own +1 stake (taken at attach, released by
/// the dispatcher) — `result` too: the attach site's caller may
/// discard the returned promise before the microtask fires, so the
/// arg keeps it alive. `source` is the record's back-pointer, a
/// stake only once the job is queued (torajs-promise
/// `reaction::record`, which [`THEN_ANY_OPS`] opts into). `is_finally`
/// switches the dispatcher to the §27.2.5.3 shape: `on_ok` runs on
/// EITHER settlement, argument-free, return ignored, settlement
/// forwarded (a callback throw still wins as the rejection).
#[repr(C)]
pub(super) struct ThenAnyArg {
    pub(super) source: *mut c_void,
    pub(super) on_ok: *mut c_void,
    pub(super) on_ok_entry: u64,
    pub(super) on_err: *mut c_void,
    pub(super) on_err_entry: u64,
    pub(super) result: *mut c_void,
    pub(super) is_finally: i64,
}

unsafe extern "C" {
    fn __torajs_promise_attach_then_ops(
        source: *mut c_void,
        invoke: unsafe extern "C" fn(i64),
        arg: i64,
        ops: *const ReactionOps,
    );
}

/// torajs-promise `reaction::record::ReactionOps` (lockstep).
#[repr(C)]
struct ReactionOps {
    trace: unsafe extern "C" fn(i64, TraceVisit, *mut c_void),
    release: unsafe extern "C" fn(i64),
}

type TraceVisit = unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void);

static THEN_ANY_OPS: ReactionOps = ReactionOps {
    trace: then_any_trace,
    release: then_any_release,
};

/// Park the reaction on `source`.
pub(super) unsafe fn attach(source: *mut c_void, a: *mut ThenAnyArg) {
    unsafe { __torajs_promise_attach_then_ops(source, then_any_dispatch, a as i64, &THEN_ANY_OPS) }
}

/// The cells the block holds a stake on, for the cycle collector.
unsafe extern "C" fn then_any_trace(arg: i64, visit: TraceVisit, ctx: *mut c_void) {
    let a = arg as *mut ThenAnyArg;
    unsafe {
        for slot in [
            &raw mut (*a).on_ok,
            &raw mut (*a).on_err,
            &raw mut (*a).result,
        ] {
            if !(*slot).is_null() {
                visit(0, *slot, slot as *mut c_void, ctx);
            }
        }
    }
}

/// The source died with the reaction parked: release the block. A
/// slot the collector zeroed was accounted for there.
unsafe extern "C" fn then_any_release(arg: i64) {
    let a = arg as *mut ThenAnyArg;
    unsafe {
        if !(*a).result.is_null() {
            __torajs_promise_drop((*a).result);
        }
        if !(*a).on_ok.is_null() {
            __torajs_value_drop_heap((*a).on_ok);
        }
        if !(*a).on_err.is_null() {
            __torajs_value_drop_heap((*a).on_err);
        }
        free(a as *mut c_void);
    }
}

/// Handler slot empty — forward the settlement (value shared: one
/// more owner when the slot holds a droppable stake; `rc_inc` is
/// NaN-box aware, so REPR_ANY immediates pass through it unharmed).
unsafe fn forward_settle(result: *mut c_void, rejected: bool, repr: u8, is_heap: u8, value: i64) {
    unsafe {
        if is_heap != 0 && value != 0 {
            __torajs_rc_inc(value as *mut c_void);
        }
        stamp_result(result, repr, is_heap);
        if rejected {
            __torajs_promise_reject(result, value);
        } else {
            __torajs_promise_resolve(result, value);
        }
    }
}

unsafe extern "C" fn then_any_dispatch(arg: i64) {
    let a = arg as *mut ThenAnyArg;
    unsafe {
        let src = (*a).source;
        let state = src.cast::<u8>().add(P_STATE_OFF).read();
        let repr = src.cast::<u8>().add(P_REPR_OFF).read();
        let is_heap = src.cast::<u8>().add(P_IS_HEAP_OFF).read();
        let value = (src.cast::<u8>().add(P_VALUE_OFF) as *const i64).read();
        let rejected = state == STATE_REJECTED;
        let (env, entry) = if rejected {
            ((*a).on_err, (*a).on_err_entry)
        } else {
            ((*a).on_ok, (*a).on_ok_entry)
        };
        let result = (*a).result;
        let mut handed_off = false;
        if (*a).is_finally != 0 {
            // §27.2.5.3 — the callback runs argument-free on either
            // settlement; a throw wins as the rejection; a returned
            // promise is waited on (`promiseResolve(C, r).then(thunk)`
            // — its rejection displaces the settlement, its fulfilment
            // forwards the source's), and a returned promise whose
            // `then` is not the builtin's is Invoked, closures and
            // all; any other return is dropped and the settlement
            // forwards. The wait hands the source / result stakes to
            // the job, so the tail's drops are skipped for it.
            let cb = (*a).on_ok;
            let ret = if cb.is_null() {
                VALUE_UNDEFINED
            } else {
                invoke_boxed(cb, (*a).on_ok_entry, core::ptr::null(), 0)
            };
            if __torajs_throw_check() != 0 {
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                let reason = __torajs_anyv_box_from_pair(ttag, tval);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            } else if crate::method_call_promise_invoke::is_promise_cell(ret) {
                crate::method_call_promise_invoke::finally_returned_promise(
                    src,
                    crate::nanbox::as_void_ptr(ret),
                    result,
                );
                handed_off = true;
            } else {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(ret);
                forward_settle(result, rejected, repr, is_heap, value);
            }
        } else if env.is_null() {
            forward_settle(result, rejected, repr, is_heap, value);
        } else {
            let cb_argv = [box_settled(repr, value)];
            let ret = invoke_boxed(env, entry, cb_argv.as_ptr(), 1);
            if __torajs_throw_check() != 0 {
                // §27.2.2.1 PromiseReactionJob abrupt completion —
                // the callback's throw becomes the result's
                // rejection (take order: tag peek before value).
                let ttag = __torajs_throw_take_tag();
                let tval = __torajs_throw_take();
                let reason = __torajs_anyv_box_from_pair(ttag, tval);
                stamp_result(result, REPR_ANY, 1);
                __torajs_promise_reject(result, reason as i64);
            } else {
                resolve_result_with(result, ret);
            }
        }
        if !handed_off {
            __torajs_promise_drop(src);
            __torajs_promise_drop(result);
        }
        if !(*a).on_ok.is_null() {
            __torajs_value_drop_heap((*a).on_ok);
        }
        if !(*a).on_err.is_null() {
            __torajs_value_drop_heap((*a).on_err);
        }
        free(a as *mut c_void);
    }
}
