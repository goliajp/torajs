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
//! composition, boxed, settled with `REPR_ANY`.
//!
//! `Promise.allSettled` stays on the knife-A reject-only path: the
//! sync kernel has no any-lane sibling yet (its `{status, value}`
//! result struct walks raw promise pointers — the recorded L3b
//! follow-up), so the checker admits only unambiguously
//! non-iterable primitives for it and the dyn entry only ever
//! rejects.

use core::ffi::{c_char, c_void};

use crate::layout::{ARR_DATA_PTR_OFF, ARR_HEAD_OFF, REPR_ANY, STATE_REJECTED};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
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
unsafe fn reject_with_pending_throw() -> *mut c_void {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let boxed = __torajs_anyv_box_from_pair(tag, value);
        crate::combinator::defer_settle(STATE_REJECTED, boxed as i64, 1, REPR_ANY)
    }
}

/// Mint a TypeError and answer a promise rejected with it — the
/// knife-A path, still the whole story for `allSettled`.
unsafe fn reject_not_iterable() -> *mut c_void {
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
        reject_with_pending_throw()
    }
}

/// §7.4.2 GetIterator + full drive: collect every element the
/// iterable yields into a fresh any-shape array (owned slots), or
/// answer the rejected promise when any step leaves a throw in
/// flight. The receiver is borrowed; the iterator ref lives in
/// `iter_slot` and is released on every exit (nanbox-aware dec — an
/// undefined immediate is a no-op).
unsafe fn collect_iterable(v: u64) -> Result<*mut c_void, *mut c_void> {
    unsafe {
        let undef = __torajs_anyv_box_from_pair(5, 0);
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = undef;
        let mut out_v: u64 = undef;
        let mut items: Vec<u64> = Vec::new();
        loop {
            let has = __torajs_any_iter_next(v, &mut idx, &mut iter_slot, &mut out_v);
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

/// # Safety
/// `v` is a live any-boxed value the caller owns for the duration
/// of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_dyn(v: u64) -> *mut c_void {
    unsafe { dyn_combinator(v, crate::combinator::__torajs_promise_all_sync) }
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

/// # Safety
/// See [`__torajs_promise_all_dyn`]. Reject-only (see module doc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_dyn(_v: u64) -> *mut c_void {
    unsafe { reject_not_iterable() }
}
