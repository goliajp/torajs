//! Promise.all / race / any sync combinators (`allSettled` lives in
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
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, ARR_LEN_OFF, Promise, REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP,
    REPR_I64, REPR_STR, REPR_UNSTAMPED, REPR_VOID, STATE_FULFILLED, STATE_REJECTED, as_promise,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// Universal heap drop — releases the deferred-settle queue
    /// stake after the dispatcher fires.
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);

    /// Array<i64> alloc + push, defined in libtorajs_arr.a — how
    /// Promise.all builds its result Array.
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    /// torajs-arr — elem-kind self-description (RFC 20260704 S1);
    /// the result array is any-consumable via the settled cell's
    /// REPR_HEAP stamp, so it needs the mark too.
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);

}

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
// Promise.all<T>(Promise<T>[]) → Promise<T[]>
// ============================================================

/// Map a source promise's value repr onto the result array's
/// elem-kind chain (torajs-rc `arr_kind` numbering: 1=I64, 2=F64,
/// 3=BOOL, 4=heap-cell). `None` = a form the raw-slot array cannot
/// self-describe (AnyValue bits / void / null fillers) — the caller
/// leaves the settled cell UNSTAMPED so the any lane stays loud
/// instead of misdecoding slots.
pub(crate) fn repr_arr_kind_chain(repr: u8) -> Option<u64> {
    match repr {
        REPR_I64 => Some(1),
        REPR_F64 => Some(2),
        REPR_BOOL => Some(3),
        REPR_STR | REPR_HEAP => Some(4),
        _ => None,
    }
}

/// `Some(lane)` when this element settled through the `any` world and
/// has to be unboxed into `lane` before it can sit in the result array;
/// `None` when its slot is already in the form the array holds.
///
/// An executor-minted cell (`new Promise((res) => res(v))`) settles
/// through the any lane, so its slot honestly holds a NaN box and its
/// stamp honestly says `REPR_ANY`. That form has no raw-slot array
/// shape: `repr_arr_kind_chain` answers `None` for it, the result gets
/// stamped UNSTAMPED, and the any-param handler refuses it — correctly,
/// because the array really was undescribed. The typed side was worse
/// and quieter: the slots held box bits, so a static `a[0] + a[1]` read
/// them as f64 and answered NaN.
///
/// Only the CALL SITE knows what the array's elements are supposed to
/// be — `Promise.all`'s result type is `Promise<T[]>` in the checker,
/// while SSA's `Type::Promise` is inner-erased — so it hands the target
/// form down, the way `settle_param` and `__torajs_promise_get_value_as`
/// already take the lane they are feeding. A `target_repr` of 0 means
/// the site could not name one, which keeps the old behaviour exactly.
#[inline]
pub(crate) fn unbox_target(src_repr: u8, target_repr: u8) -> Option<u8> {
    if src_repr != REPR_ANY || target_repr == REPR_UNSTAMPED || target_repr == REPR_ANY {
        return None;
    }
    Some(target_repr)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_sync(
    promises_arr: *mut c_void,
    target_repr: i64,
) -> *mut c_void {
    let target_repr = target_repr as u8;
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    if unsafe { sparse_input_rejects(promises_arr) } {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // §27.2.4 static-slot patch consult (rotation 448) — a user
    // override on `Promise.resolve` detours to the per-element
    // Call(promiseResolve, C, «v») lane; see `combinator_patched`.
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_all(promises_arr) };
    }
    // An `Array<Any>` input carries NaN-box slots (mixed promise /
    // plain-value elements) — the raw-pointer walk below would
    // dereference immediates; route to the any-lane sibling.
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { crate::combinator_any::all_sync_any(promises_arr) };
    }
    // A genuinely pending element is the one thing the walk below
    // cannot answer: it used to reject the result with a placeholder,
    // which is how the commonest shape in the family
    // (`Promise.all([asyncCall(), asyncCall()])`) became an uncaught
    // rejection. The fan-in waits instead. The all-settled input keeps
    // this walk — its microtask position is what the existing fixtures
    // encode, and routing it through jobs would move ticks for nothing.
    if unsafe { crate::combinator_all_fanin::has_pending(promises_arr) } {
        return unsafe { crate::combinator_all_fanin::all_fan_in(promises_arr, target_repr) };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    // Pre-scan: first rejected → reject outer with that reason. The
    // element form is read here as well, so the build loop below
    // already knows whether the result array will co-own its slots
    // before it starts filling them. The typed tier guarantees a
    // homogeneous `Array<Promise<T>>` input, so the first element's
    // form describes all of them.
    let mut elem_repr = REPR_UNSTAMPED;
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        let state = unsafe { (*pp).state };
        if state == STATE_REJECTED {
            return unsafe { defer_settle(STATE_REJECTED, (*pp).value, 0, (*pp).value_repr) };
        }
        if elem_repr == REPR_UNSTAMPED {
            let src = unsafe { (*pp).value_repr };
            elem_repr = unbox_target(src, target_repr).unwrap_or(src);
        }
    }
    // All fulfilled. The call site typed the result element `any`, so
    // the elements share no raw form and the result has to carry
    // NaN-box slots — the shape an any-shape INPUT already produces,
    // reached here for the other reason (`AllBlock::result_any` names
    // both). Without this the loop below picked ONE element's form and
    // read every slot through it, which is silent for a heterogeneous
    // input: a Str pointer read as a number, `true` read as 1.
    if target_repr == REPR_ANY {
        return unsafe { crate::combinator_any::all_sync_boxed_from_typed(promises_arr, len) };
    }
    let chain = repr_arr_kind_chain(elem_repr);
    // A heap-chained result array DROPS every non-null slot once its
    // last owner dies (`__torajs_arr_drop_heap` walks them all), while
    // each source promise keeps its own stake — so every element the
    // result co-owns has to be paid for here. Without this the sources
    // died first and freed the very cells the result still pointed at,
    // and the reads came back as whatever later reused those blocks:
    // SILENTLY wrong values rather than a crash. `allSettled`'s
    // inner-value inc and `race`/`any`'s forwarded-value inc are the
    // same payment; only this loop was missing it.
    let co_owns = chain == Some(4);
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        let v = if pp.is_null() {
            0
        } else {
            unsafe {
                match unbox_target((*pp).value_repr, target_repr) {
                    Some(lane) => crate::then_box::unbox_settled(lane, (*pp).value),
                    None => (*pp).value,
                }
            }
        };
        if co_owns && v != 0 {
            unsafe { __torajs_rc_inc(v as *mut c_void) };
        }
        result_arr = unsafe { __torajs_arr_push(result_arr, v) };
    }
    let out_repr = match chain {
        Some(c) => {
            unsafe { __torajs_arr_mark_kind(result_arr, c) };
            REPR_HEAP
        }
        // Unmarkable slots — keep the settled cell loud rather than
        // hand the any lane a misdecoding array.
        None => REPR_UNSTAMPED,
    };
    unsafe { settle_result(len, STATE_FULFILLED, result_arr as i64, 1, out_repr) }
}

// ============================================================
// Promise.race — first settled wins
// ============================================================

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
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_race(promises_arr) };
    }
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { crate::combinator_any::race_sync_any(promises_arr) };
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
        let result = crate::pool::__torajs_promise_alloc_pending();
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
            crate::then_adopt::adopt_into(result, pp as *mut c_void);
        }
        result
    }
}

// ============================================================
// Promise.any — first FULFILLED wins; all-rejected → last reason
// ============================================================

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
    if unsafe { crate::combinator_patched::consult_active() } {
        return unsafe { crate::combinator_patched::run_any(promises_arr) };
    }
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { crate::combinator_any::any_sync_any(promises_arr) };
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
