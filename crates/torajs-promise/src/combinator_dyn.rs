//! Promise combinators over a dynamically-typed (any-boxed)
//! argument — RFC 20260730-promise-combinator-iterable knives A+B.
//!
//! ES §27.2.4.{1,3,5}: `Promise.all/any/race(arg)` runs GetIterator
//! on `arg`. tr's static tier only ever admitted
//! `Array<Promise<T>>` / `Array<Any>`; every other argument type
//! was a whole-program compile reject, where the spec wants runtime
//! behavior — iterate whatever GetIterator yields, and answer a
//! promise REJECTED with the TypeError when it throws.
//!
//! The dynamic entries are one collect-then-delegate: drive
//! `__torajs_any_iter_next` (the for-of any-lane protocol — arrays,
//! strings per code unit, Map/Set defaults, iterator cells, class
//! instances with a real `[Symbol.iterator]()`, and a catchable
//! TypeError on a non-iterable) to collect the elements into an
//! any-shape array, then hand that array to the matching `*_sync`
//! kernel, whose entry already routes `FLAG_ARR_ANY` inputs to the
//! any-lane sibling. A pending throw at any step (GetIterator miss,
//! poisoned `next()`, a step body throw) becomes the rejection
//! reason — popped off the throw TLS by the take_tag + take
//! composition, boxed, settled with `REPR_ANY`. All four
//! combinators share the shape — `allSettled` joined once its
//! any-lane sibling (`combinator_any::allsettled_sync_any`) landed.

use core::ffi::c_void;

use crate::layout::{ARR_DATA_PTR_OFF, ARR_HEAD_OFF, REPR_ANY, STATE_REJECTED};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
    fn __torajs_any_iter_next(
        recv: u64,
        idx_slot: *mut i64,
        iter_slot: *mut u64,
        out: *mut u64,
    ) -> i64;
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Pop the in-flight throw off the TLS and answer a promise rejected
/// with it (take_tag peeks first; take clears `active` but leaves
/// the tag slot — the `: any`-typed catch order).
pub(crate) unsafe fn reject_with_pending_throw() -> *mut c_void {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let boxed = __torajs_anyv_box_from_pair(tag, value);
        crate::combinator::defer_settle(STATE_REJECTED, boxed as i64, 1, REPR_ANY)
    }
}

/// The `__torajs_any_iter_next` family's shared signature — the
/// for-of any-lane step protocol (receiver, index slot, iterator
/// ref slot, out slot) → has-element flag.
pub(crate) type IterStepFn = unsafe extern "C" fn(u64, *mut i64, *mut u64, *mut u64) -> i64;

/// §7.4.2 GetIterator + full drive: collect every element the step
/// protocol yields into an owned Vec, or answer the rejected promise
/// when any step leaves a throw in flight. The step fn picks the
/// non-iterable verdict: `any_iter_next` throws TypeError, the
/// `_array_like` sibling walks `length` + index keys (§23.1.2.1
/// step 3's branch, resolved in the kernel). The receiver is
/// borrowed; the iterator ref lives in `iter_slot` and is released
/// on every exit (nanbox-aware dec — an undefined immediate is a
/// no-op).
pub(crate) unsafe fn collect_items(v: u64, step: IterStepFn) -> Result<Vec<u64>, *mut c_void> {
    unsafe {
        let undef = __torajs_anyv_box_from_pair(5, 0);
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = undef;
        let mut out_v: u64 = undef;
        let mut items: Vec<u64> = Vec::new();
        loop {
            let has = step(v, &mut idx, &mut iter_slot, &mut out_v);
            if __torajs_throw_check() != 0 {
                for it in items {
                    __torajs_anyv_rc_dec(it);
                }
                __torajs_anyv_rc_dec(iter_slot);
                return Err(reject_with_pending_throw());
            }
            if has == 0 {
                break;
            }
            // The step's rc is the caller's (the for-of ledger) —
            // the collected slot keeps it.
            items.push(out_v);
        }
        __torajs_anyv_rc_dec(iter_slot);
        Ok(items)
    }
}

/// Collect via the strict-iterable step, then build the any-shape
/// array the `*_sync` kernels consume (owned slot transfer).
unsafe fn collect_iterable(v: u64) -> Result<*mut c_void, *mut c_void> {
    unsafe {
        let items = collect_items(v, __torajs_any_iter_next)?;
        let out = __torajs_arr_alloc_any_filled(items.len() as u64);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for (i, bits) in items.into_iter().enumerate() {
            // Owned transfer — the prefilled undefined is an
            // immediate, overwriting it leaks nothing.
            *(data.add((head as usize + i) * 8) as *mut u64) = bits;
        }
        Ok(out as *mut c_void)
    }
}

/// Collect, delegate to the sync kernel (its entry routes the
/// any-shape array to the any-lane sibling), then drop the temp
/// array — the kernel borrows it and incs what it keeps.
unsafe fn dyn_combinator(
    v: u64,
    sync: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
) -> *mut c_void {
    unsafe {
        match collect_iterable(v) {
            Err(rejected) => rejected,
            Ok(arr) => {
                let out = sync(arr);
                __torajs_value_drop_heap(arr);
                out
            }
        }
    }
}

/// `all_sync` takes the result array's target element form from its
/// call site; the dyn entry has none to give. Everything it collects is
/// an any-shape array, which `all_sync` hands to its any-lane sibling
/// before that form would be consulted, so `0` ("the site could not
/// name a lane") is the honest word here rather than a lost opportunity.
pub(crate) unsafe extern "C" fn all_sync_untargeted(arr: *mut c_void) -> *mut c_void {
    unsafe { crate::combinator::__torajs_promise_all_sync(arr, 0) }
}

/// # Safety
/// `v` is a live any-boxed value the caller owns for the duration
/// of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_dyn(v: u64) -> *mut c_void {
    unsafe { dyn_combinator(v, all_sync_untargeted) }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_race_dyn(v: u64) -> *mut c_void {
    unsafe { dyn_combinator(v, crate::combinator::__torajs_promise_race_sync) }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_any_dyn(v: u64) -> *mut c_void {
    unsafe { dyn_combinator(v, crate::combinator::__torajs_promise_any_sync) }
}

/// Same shape as [`all_sync_untargeted`]: `allsettled_sync` takes the
/// class tags its records must carry and the form their value slot
/// holds, and the dyn entry has no call site to derive either from.
/// Two zeros leave the records anonymous with their slots untouched —
/// the posture every allSettled record had before those words existed.
pub(crate) unsafe extern "C" fn allsettled_sync_untagged(arr: *mut c_void) -> *mut c_void {
    unsafe { crate::combinator_allsettled::__torajs_promise_allsettled_sync(arr, 0, 0) }
}

/// # Safety
/// See [`__torajs_promise_all_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_dyn(v: u64) -> *mut c_void {
    unsafe { dyn_combinator(v, allsettled_sync_untagged) }
}
