//! The combinator result's SETTLE posture — split from
//! [`crate::combinator`] when the §27.2.4 patch gates pushed it over
//! the 500-line cap (rotation 448). The input-reading walks stay
//! there; this file answers how a combinator's RESULT reaches its
//! settled state on the spec's microtask position (deferred one tick
//! for a non-empty input, synchronous for the empty one).

use core::ffi::c_void;

use crate::layout::{REPR_ANY, REPR_HEAP, STATE_REJECTED, as_promise};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
}

// §27.2.1.3 steps 2.f-2.l over an already pending cell (torajs-
// anyvalue): reads `then` off the value, and either rejects with an
// abrupt read or enqueues the NewPromiseResolveThenableJob. Answers 1
// when it took the value (consuming its stake), 0 to let the caller
// settle as before. Reached through a seam whose default lives in
// torajs-dispatch: an artifact whose only combinator entry is
// `Promise.all`'s proved one stands on the `then` closure proof, so no
// result it settles can have a `then`, and the link shadows the seam
// with a stub that answers 0. Unit tests carry no dispatch member, so
// they bridge directly.
#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_combinator_result_thenable_slow(result: *mut c_void, repr: i64, value: i64) -> i64;
}
#[cfg(test)]
unsafe fn __torajs_combinator_result_thenable_slow(
    result: *mut c_void,
    repr: i64,
    value: i64,
) -> i64 {
    unsafe extern "C" {
        fn __torajs_anyv_promise_settle_thenable(result: *mut c_void, repr: i64, value: i64)
        -> i64;
    }
    unsafe { __torajs_anyv_promise_settle_thenable(result, repr, value) }
}

/// §27.2.1.3 step 2.f at a combinator's RESULT — the capability's
/// resolve function reads `then` off whatever it is handed, and a
/// combinator hands it an array.
///
/// In every ordinary program that array is a plain array and this
/// costs one property lookup per combinator call. But
/// `Array.prototype.then` makes it a thenable, and the spec adopts it
/// — `Promise.all([])` answers what that `then` resolves with, not
/// the array. tr fulfilled with the array, and when the `then` read
/// THREW it fulfilled anyway where the spec rejects, which is a
/// failure turned into a success.
///
/// Ownership is the whole difficulty. The cell already holds the
/// value and owes its stake, while the hook CONSUMES that stake when
/// it adopts. So the cell hands ownership over before the call and is
/// left holding nothing; if the hook declines (a plain array, the
/// common case), the value goes back exactly as it was. Getting this
/// wrong is a double free at the cell's own drop, not a visible bug.
unsafe fn result_adopted_as_thenable(p: *mut c_void, repr: u8, value: i64, is_heap: u8) -> bool {
    if is_heap == 0 || !matches!(repr, REPR_HEAP | REPR_ANY) {
        return false;
    }
    unsafe {
        let pp = as_promise(p);
        (*pp).value = 0;
        (*pp).value_is_heap = 0;
        if __torajs_combinator_result_thenable_slow(p, repr as i64, value) != 0 {
            return true;
        }
        (*pp).value = value;
        (*pp).value_is_heap = is_heap;
        false
    }
}

/// Deferred-settle microtask (L3b combinator residual face ② fix).
/// Reads the target state the mint parked in `deferred_state`, clears it,
/// and routes through the regular resolve/reject kernels — reject
/// keeps its PENDING→REJECTED HPRT-check enqueue, and both drain any
/// callbacks attached during the deferral round. The queue's stake
/// (inc'd at enqueue) releases on exit.
unsafe extern "C" fn deferred_settle_dispatch(arg: i64) {
    let p = arg as *mut c_void;
    let pp = as_promise(p);
    unsafe {
        let target = (*pp).deferred_state;
        (*pp).deferred_state = 0;
        let v = (*pp).value;
        if target == STATE_REJECTED {
            // Rejection performs no thenable absorption (§27.2.1.3.1
            // is the resolve side only) — a promise reason included.
            crate::state::__torajs_promise_reject(p, v);
        } else if !result_adopted_as_thenable(p, (*pp).value_repr, v, (*pp).value_is_heap) {
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
/// (`deferred_state` carries the target state; resolve/reject never touch
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
        let p = crate::pool::alloc_pending();
        let pp = as_promise(p);
        (*pp).value = value;
        (*pp).value_is_heap = is_heap;
        (*pp).value_repr = repr;
        (*pp).deferred_state = state;
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
        // The empty answer goes through the same `then` read, and it
        // is the shape that actually reaches it: `Promise.all([])`
        // resolves with a freshly built array, which
        // `Array.prototype.then` turns into a thenable. Minting
        // PENDING and settling in place keeps the synchronous tick
        // this function exists to preserve — `resolve` on a cell with
        // no callbacks attached is the same transition
        // `alloc_fulfilled` performs.
        if state != STATE_REJECTED && is_heap != 0 && matches!(repr, REPR_HEAP | REPR_ANY) {
            let p = crate::pool::alloc_pending();
            let pp = as_promise(p);
            (*pp).value_repr = repr;
            (*pp).value_is_heap = is_heap;
            if __torajs_combinator_result_thenable_slow(p, repr as i64, value) != 0 {
                return p;
            }
            crate::state::__torajs_promise_resolve(p, value);
            return p;
        }
        let p = match (state, is_heap) {
            (STATE_REJECTED, 0) => crate::pool::__torajs_promise_alloc_rejected(value),
            (STATE_REJECTED, _) => crate::pool::__torajs_promise_alloc_rejected_heap(value),
            (_, 0) => crate::pool::alloc_fulfilled(value),
            (_, _) => crate::pool::alloc_fulfilled_heap(value),
        };
        (*as_promise(p)).value_repr = repr;
        p
    }
}
