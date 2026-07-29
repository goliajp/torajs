//! `Array.fromAsync(items)` — proposal-array-from-async §2.1.1,
//! sync-source MVP.
//!
//! The dynamic entry drives `__torajs_any_iter_next_array_like`
//! (the `Array.from` step kernel: a spec-iterable walks its
//! GetIterator protocol, anything else takes the §23.1.2.1 step-3
//! array-like branch — `length` + index keys, so a plain number
//! answers `[]` and `null` throws the length-read TypeError), then
//! settles each collected element: spec step 5.e awaits every
//! element, so a settled promise element unwraps to its value per
//! its repr stamp and a rejected one rejects the whole result. The
//! result rides a `REPR_HEAP` promise holding an `Array<Any>`.
//!
//! Recorded MVP boundaries (registered, spec-strict forms follow):
//! a real async iterator (`Symbol.asyncIterator`) is not consulted —
//! the sync step protocol answers for it; a PENDING or UNSTAMPED
//! promise element takes the combinator placeholder-reject posture;
//! non-promise thenables store verbatim (no generic `then` await);
//! the `mapFn` arity stays a loud compile reject.

use core::ffi::c_void;

use crate::combinator_any::{box_settled_owned, slot_promise};
use crate::combinator_dyn::collect_items;
use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, REPR_ANY, REPR_HEAP, REPR_UNSTAMPED, REPR_VOID,
    STATE_FULFILLED, STATE_PENDING, STATE_REJECTED,
};

unsafe extern "C" {
    fn __torajs_any_iter_next_array_like(
        recv: u64,
        idx_slot: *mut i64,
        iter_slot: *mut u64,
        out: *mut u64,
    ) -> i64;
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_rc_dec(v: u64);
}

/// # Safety
/// `v` is a live any-boxed value the caller owns for the duration
/// of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_array_from_async_dyn(v: u64) -> *mut c_void {
    unsafe {
        let items = match collect_items(v, __torajs_any_iter_next_array_like) {
            Err(rejected) => return rejected,
            Ok(items) => items,
        };
        // Pre-scan (spec step 5.e award order): the first rejected
        // element's reason rejects the whole result; a PENDING or
        // UNSTAMPED element takes the MVP placeholder reject. Both
        // verdicts release every collected item before answering,
        // so the build loop below never strands a half-owned Vec.
        for &bits in &items {
            let Some(pp) = slot_promise(bits) else {
                continue;
            };
            (*pp).has_handler = 1;
            let state = (*pp).state;
            let verdict = if state == STATE_REJECTED {
                match box_settled_owned((*pp).value_repr, (*pp).value) {
                    Some(reason) => {
                        crate::combinator::defer_settle(STATE_REJECTED, reason as i64, 1, REPR_ANY)
                    }
                    None => crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID),
                }
            } else if state == STATE_PENDING || (*pp).value_repr == REPR_UNSTAMPED {
                crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID)
            } else {
                continue;
            };
            for it in items {
                __torajs_anyv_rc_dec(it);
            }
            return verdict;
        }
        // All plain or fulfilled — build the Array<Any> result. A
        // fulfilled promise element unwraps to a fresh owned box and
        // the collected promise reference releases; a plain element
        // transfers its collected stake verbatim.
        let out = __torajs_arr_alloc_any_filled(items.len() as u64);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for (i, bits) in items.into_iter().enumerate() {
            let v = match slot_promise(bits) {
                // The pre-scan gated UNSTAMPED — the fallback keeps
                // this total without a runtime panic path (the
                // combinator_any build-loop posture).
                Some(pp) => {
                    let unwrapped = box_settled_owned((*pp).value_repr, (*pp).value)
                        .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0));
                    __torajs_anyv_rc_dec(bits);
                    unwrapped
                }
                None => bits,
            };
            *(data.add((head as usize + i) * 8) as *mut u64) = v;
        }
        crate::combinator::defer_settle(STATE_FULFILLED, out as i64, 1, REPR_HEAP)
    }
}
