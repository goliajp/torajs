//! The combinator result's SETTLE posture — split from
//! [`crate::combinator`] when the §27.2.4 patch gates pushed it over
//! the 500-line cap (rotation 448). The input-reading walks stay
//! there; this file answers how a combinator's RESULT reaches its
//! settled state on the spec's microtask position (deferred one tick
//! for a non-empty input, synchronous for the empty one).

use core::ffi::c_void;

use crate::layout::{STATE_REJECTED, as_promise};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
}

/// Deferred-settle microtask (L3b combinator residual face ② fix).
/// Reads the target state the mint parked in `_pad[0]`, clears it,
/// and routes through the regular resolve/reject kernels — reject
/// keeps its PENDING→REJECTED HPRT-check enqueue, and both drain any
/// callbacks attached during the deferral round. The queue's stake
/// (inc'd at enqueue) releases on exit.
unsafe extern "C" fn deferred_settle_dispatch(arg: i64) {
    let p = arg as *mut c_void;
    let pp = as_promise(p);
    unsafe {
        let target = (*pp)._pad[0];
        (*pp)._pad[0] = 0;
        let v = (*pp).value;
        if target == STATE_REJECTED {
            crate::state::__torajs_promise_reject(p, v);
        } else {
            crate::state::__torajs_promise_resolve(p, v);
        }
        __torajs_value_drop_heap(p);
    }
}

/// Mint the combinator's result promise: PENDING now, settled one
/// microtask later. The spec shape settles combinator results
/// through their resolve functions' absorption round — the sync
/// fast path used to mint them already-settled, which put their
/// `.then` callbacks one microtask EARLY relative to bun (output-
/// order divergence, L3b combinator residual face ②). The final
/// (state, value, is_heap) rides pre-written on the pending cell
/// (`_pad[0]` carries the target state; resolve/reject never touch
/// `value_is_heap`/`value_repr`, so the pre-writes survive), and
/// `await` still observes the settled value because its lowering
/// drains the microtask queue before `promise_get_value`.
///
/// Also carries the knife-3 (RFC 20260720-anylane-promise-methods)
/// value-form stamp: an Arr result is REPR_HEAP, a forwarded
/// settlement copies its source's stamp, the MVP placeholder
/// reject(0) legs answer undefined (REPR_VOID).
pub(crate) unsafe fn defer_settle(state: u8, value: i64, is_heap: u8, repr: u8) -> *mut c_void {
    unsafe {
        let p = crate::pool::__torajs_promise_alloc_pending();
        let pp = as_promise(p);
        (*pp).value = value;
        (*pp).value_is_heap = is_heap;
        (*pp).value_repr = repr;
        (*pp)._pad[0] = state;
        // Queue stake — the cell must outlive the deferral even if
        // the caller discards the result; the dispatcher drops it.
        __torajs_rc_inc(p);
        __torajs_microtask_enqueue(deferred_settle_dispatch, p as i64);
        p
    }
}

/// Settle a combinator's result: an EMPTY input synchronously,
/// anything else through [`defer_settle`].
///
/// The deferral exists because the spec settles a non-empty
/// combinator through a round of element jobs, so minting one settled
/// put its callbacks a microtask EARLY. An empty iterable has no
/// elements and no jobs — §27.2.4.1 step 8 and §27.2.4.2 step 8 reach
/// remainingElementsCount 0 before the call returns, and bun settles
/// there and then. Routing it through the same deferral was the
/// mirror error, one microtask LATE: `Promise.all([]).then(cb)` ran
/// `cb` after a plain `Promise.resolve(0).then(t1)` where bun runs it
/// before. Probed on `all` / `allSettled` / `any` alike, which is why
/// the rule lives here rather than in one kernel.
///
/// `race` has no empty answer to settle — §27.2.4.5 step 3 leaves it
/// forever pending — so it never comes through here.
pub(crate) unsafe fn settle_result(
    len: u64,
    state: u8,
    value: i64,
    is_heap: u8,
    repr: u8,
) -> *mut c_void {
    unsafe {
        if len != 0 {
            return defer_settle(state, value, is_heap, repr);
        }
        let p = match (state, is_heap) {
            (STATE_REJECTED, 0) => crate::pool::__torajs_promise_alloc_rejected(value),
            (STATE_REJECTED, _) => crate::pool::__torajs_promise_alloc_rejected_heap(value),
            (_, 0) => crate::pool::__torajs_promise_alloc_fulfilled(value),
            (_, _) => crate::pool::__torajs_promise_alloc_fulfilled_heap(value),
        };
        (*as_promise(p)).value_repr = repr;
        p
    }
}
