//! Promise.race / any sync combinators (`all` lives in
//! [`crate::combinator_all_sync`], `allSettled` in
//! [`crate::combinator_allsettled`]).
//!
//! Port of `runtime_promise.c` T-17.a, T-17.b, T-17.d sections (P6.1,
//! 2026-05-24). Each kernel here answers an input whose elements have
//! all settled, and hands a pending one to the fan-in next door:
//! `race` through one adopt job per element (it needs neither a
//! counter nor a result array), `all` / `allSettled` / `any` through
//! the shared counter block.
//!
//! Array layout reads use the raw byte-offset accessors from
//! `crate::layout` — torajs-promise carries its own knowledge of
//! the Array<T> 8B-stride layout rather than depending on
//! torajs-arr (mirrors the C source's pattern of independent
//! layout knowledge in this section).

use core::ffi::c_void;

use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, ARR_LEN_OFF, Promise, REPR_HEAP, REPR_VOID, STATE_FULFILLED,
    STATE_REJECTED, as_promise,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// Universal heap drop — releases the deferred-settle queue
    /// stake after the dispatcher fires.
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
}

pub(crate) use crate::combinator_all_sync::repr_arr_kind_chain;
pub(crate) use crate::combinator_sparse::sparse_input_rejects;
// The settle posture lives next door (rotation 448 file split); the
// re-export keeps every `crate::combinator::{defer_settle,
// settle_result}` caller's path canonical.
pub(crate) use crate::combinator_settle::{defer_settle, settle_result};

/// Read logical Array<T> slot `i` from `arr` (8B stride; pointer-
/// shape values stored as raw bits).
#[inline]
pub(crate) unsafe fn arr_slot_ptr(arr: *mut c_void, i: u64) -> *mut Promise {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(bytes.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        let slot_off = (head + i) * 8;
        *(data.add(slot_off as usize) as *mut *mut Promise)
    }
}

#[inline]
pub(crate) unsafe fn arr_len(arr: *mut c_void) -> u64 {
    unsafe { *((arr as *mut u8).add(ARR_LEN_OFF) as *const u64) }
}

/// Mark every input promise as handled — the spec equivalent of the
/// per-input resolve/reject-element handler attach every combinator
/// performs (§27.2.4.1.2 etc.). Without this an input the combinator
/// absorbed (e.g. a REJECTED slot `Promise.any` skips past) still
/// carried `has_handler = 0` and the HPRT-check microtask reported a
/// spurious unhandled rejection (exit 1 where bun exits 0). Pending
/// inputs are marked too — a real attach would land before their
/// later settlement.
pub(crate) unsafe fn absorb_inputs(promises_arr: *mut c_void) {
    let len = unsafe { arr_len(promises_arr) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if !pp.is_null() {
            unsafe { (*pp).has_handler = 1 };
        }
    }
}

// ============================================================
// Promise.race — first settled wins
// ============================================================

/// # Safety
/// `promises_arr` is NULL or a live array whose elements are live Promise cells or values of the declared element lane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_race_sync(promises_arr: *mut c_void) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    if unsafe { sparse_input_rejects(promises_arr) } {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // §27.2.4 static-slot patch consult (rotation 448) — a user
    // override on `Promise.resolve` detours to the per-element
    // Call(promiseResolve, C, «v») lane; see `combinator_patched`.
    if let Some(out) = unsafe {
        crate::combinator_then_override::defer_on_then_override(
            promises_arr,
            crate::combinator_all_fanin::MODE_RACE,
            __torajs_promise_race_sync,
        )
    } {
        return out;
    }
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_race(promises_arr) };
    }
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        // §27.2.4.1.2 step 5.f and siblings — every element goes
        // through `promiseResolve` BEFORE anything attaches to it, so
        // the walk below sees only promises. Without it a thenable
        // element was taken for an already-fulfilled value and handed
        // on as itself. The patched lane above does the same thing
        // with the user's override; this is its builtin twin.
        return unsafe { crate::combinator_patched::run_race_builtin(promises_arr) };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        let value_is_heap = unsafe { (*pp).value_is_heap };
        let value = unsafe { (*pp).value };
        if state == STATE_FULFILLED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { defer_settle(STATE_FULFILLED, value, 1, (*pp).value_repr) };
            }
            return unsafe { defer_settle(STATE_FULFILLED, value, 0, (*pp).value_repr) };
        }
        if state == STATE_REJECTED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { defer_settle(STATE_REJECTED, value, 1, (*pp).value_repr) };
            }
            return unsafe { defer_settle(STATE_REJECTED, value, 0, (*pp).value_repr) };
        }
    }
    // Nothing has settled yet. §27.2.4.5.1 attaches a reaction to every
    // element and lets the FIRST settlement win; the placeholder reject
    // that used to stand here was the MVP's way of saying it could not
    // wait. It can now — an adopt job per element settles the outer
    // from whichever element settles first, and the losers' jobs find
    // the cell already settled and no-op through resolve/reject's
    // PENDING guard.
    //
    // An empty iterable answers a FOREVER-PENDING promise per §27.2.4.5
    // step 3 (there is no element that could ever settle it), which the
    // loop below produces by attaching nothing — matching bun, where
    // `Promise.race([])` also just runs out of work and exits.
    unsafe { race_fan_in(promises_arr, len) }
}

/// Attach one adopt job per pending element (see the call site).
///
/// Each element is borrowed from the input array, so it takes an inc to
/// pay for the stake `adopt_into` consumes. The result is pre-stamped
/// from the first element's form for the same reason the `.then`
/// kernels pre-stamp theirs: a chained any-param attach can land before
/// this cell settles and its gate refuses an UNSTAMPED source. The
/// winning adopt job overwrites the stamp with what actually settled.
unsafe fn race_fan_in(promises_arr: *mut c_void, len: u64) -> *mut c_void {
    unsafe {
        let result = crate::pool::alloc_pending();
        // An empty iterable leaves the loop below with nothing to stamp
        // from, and the cell can never settle — so no value exists to
        // mis-box and the any-param attach gate has nothing to protect.
        // VOID keeps it quiet; UNSTAMPED would make `Promise.race([])
        // .then(cb)` throw where the spec says the handler simply never
        // runs.
        (*as_promise(result)).value_repr = REPR_VOID;
        let mut stamped = false;
        for i in 0..len {
            let pp = arr_slot_ptr(promises_arr, i);
            if pp.is_null() {
                continue;
            }
            if !stamped {
                (*as_promise(result)).value_repr = (*pp).value_repr;
                (*as_promise(result)).value_is_heap = (*pp).value_is_heap;
                stamped = true;
            }
            __torajs_rc_inc(pp as *mut c_void);
            crate::then_adopt::follow(result, pp as *mut c_void);
        }
        result
    }
}

// ============================================================
// Promise.any — first FULFILLED wins; all-rejected → last reason
// ============================================================

/// # Safety
/// `promises_arr` is NULL or a live array whose elements are live Promise cells or values of the declared element lane.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_any_sync(promises_arr: *mut c_void) -> *mut c_void {
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    if unsafe { sparse_input_rejects(promises_arr) } {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // §27.2.4 static-slot patch consult (rotation 448) — a user
    // override on `Promise.resolve` detours to the per-element
    // Call(promiseResolve, C, «v») lane; see `combinator_patched`.
    if let Some(out) = unsafe {
        crate::combinator_then_override::defer_on_then_override(
            promises_arr,
            crate::combinator_all_fanin::MODE_ANY,
            __torajs_promise_any_sync,
        )
    } {
        return out;
    }
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_any(promises_arr) };
    }
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        // §27.2.4.1.2 step 5.f and siblings — every element goes
        // through `promiseResolve` BEFORE anything attaches to it, so
        // the walk below sees only promises. Without it a thenable
        // element was taken for an already-fulfilled value and handed
        // on as itself. The patched lane above does the same thing
        // with the user's override; this is its builtin twin.
        return unsafe { crate::combinator_patched::run_any_builtin(promises_arr) };
    }
    // A genuinely pending element is the one thing the walk below
    // cannot answer — it used to make `Promise.any([asyncCall(),
    // asyncCall()])` reject with a placeholder. The fan-in waits, with
    // the counter running the other way: a fulfilment short-circuits
    // and the count that has to reach zero is of rejections.
    if unsafe { crate::combinator_all_fanin::has_pending(promises_arr) } {
        return unsafe { crate::combinator_all_fanin::any_fan_in(promises_arr) };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    let mut last_rejection: i64 = 0;
    let mut last_rejection_repr: u8 = REPR_VOID;
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        let value_is_heap = unsafe { (*pp).value_is_heap };
        let value = unsafe { (*pp).value };
        if state == STATE_FULFILLED {
            if value_is_heap != 0 {
                if value != 0 {
                    unsafe { __torajs_rc_inc(value as *mut c_void) };
                }
                return unsafe { defer_settle(STATE_FULFILLED, value, 1, (*pp).value_repr) };
            }
            return unsafe { defer_settle(STATE_FULFILLED, value, 0, (*pp).value_repr) };
        }
        if state == STATE_REJECTED {
            last_rejection = value;
            last_rejection_repr = unsafe { (*pp).value_repr };
        }
    }
    // Nothing fulfilled, and the gate above ruled out anything still
    // outstanding — §27.2.4.2's all-rejected answer.
    if let Some(err) = unsafe { any_aggregate_error(promises_arr, len) } {
        return unsafe { settle_result(len, STATE_REJECTED, err as i64, 1, REPR_HEAP) };
    }
    unsafe { defer_settle(STATE_REJECTED, last_rejection, 0, last_rejection_repr) }
}

/// Collect every element's rejection reason, in input order, into the
/// `AggregateError` §27.2.4.2 answers with. `None` = no class to build
/// one from (see [`crate::combinator_aggregate`]).
unsafe fn any_aggregate_error(promises_arr: *mut c_void, len: u64) -> Option<*mut c_void> {
    unsafe {
        let errors = crate::combinator_aggregate::alloc_errors(len);
        for i in 0..len {
            let pp = arr_slot_ptr(promises_arr, i);
            if pp.is_null() {
                continue;
            }
            // The list co-owns each reason: the element promises keep
            // their own stakes, and `box_settled_owned` incs. An
            // UNSTAMPED reason has no form to box from, and undefined
            // keeps the walk total without a panic path.
            let boxed = crate::combinator_any::box_settled_owned((*pp).value_repr, (*pp).value)
                .unwrap_or_else(|| crate::combinator_any::box_undefined());
            crate::combinator_aggregate::store_error(errors, i, boxed);
        }
        crate::combinator_aggregate::make(errors)
    }
}
