//! Real fan-in for `Promise.all` (§27.2.4.1) and `Promise.allSettled`
//! (§27.2.4.3) — the path taken when an element has not settled yet.
//!
//! The sync kernel next door answers an all-settled input by walking it
//! once, and that stays: its microtask position is what every existing
//! fixture encodes, and re-routing it through jobs would move ticks for
//! no gain. What it could not do is WAIT. A pending element used to
//! reject the result with a placeholder — the MVP's way of saying so —
//! which turned the most ordinary shape in the family
//! (`Promise.all([asyncCall(), asyncCall()])`) into an uncaught
//! rejection.
//!
//! `race` (rotation 299) fell straight out of `then_adopt::adopt_into`
//! because the first settlement wins and there is nothing to collect.
//! `all` needs the two things race does not: a count of how many
//! elements are still outstanding, and a slot per element to write into
//! by index — §27.2.4.1.3's `remainingElementsCount` and its indexed
//! resolve-element functions. Both live in a block the per-element jobs
//! share, and the ownership protocol is `adopt_into`'s: each element's
//! stake transfers into its job, the block is refcounted by the jobs
//! holding it, and the dispatcher releases both.
//!
//! `allSettled` wants the same counter and the same indexed slots and
//! differs in exactly two places: a rejected element contributes a
//! record instead of short-circuiting, and the slot holds that record
//! rather than the raw value. It therefore shares this block rather
//! than growing a second copy of the protocol — four combinators times
//! two lanes times a job per element is the largest refcount surface
//! this crate has, and it is worth keeping in one place.
//!
//! `any` is the same machinery read in a mirror. A FULFILMENT is what
//! short-circuits, the count that has to reach zero is of rejections,
//! and the indexed slots hold the `errors` list §27.2.4.2's
//! AggregateError carries — pre-sized and written by index because
//! §27.2.4.2.1 stores at `errors[index]` and that order is observable.
//! Sharing the block is what makes the mirror obvious; a second copy
//! would have made it look like a different algorithm.

use core::ffi::c_void;

use crate::combinator::{absorb_inputs, arr_len, arr_slot_ptr};
use crate::layout::{
    Promise, REPR_ANY, REPR_HEAP, REPR_UNSTAMPED, STATE_FULFILLED, STATE_REJECTED, as_promise,
};
use crate::pool::__torajs_promise_drop;
use crate::state::__torajs_promise_attach_then;

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// `Promise.all` — a rejected element settles the result at once.
pub(crate) const MODE_ALL: u8 = 0;
/// `Promise.allSettled` — nothing short-circuits; every element
/// contributes a `{status, value}` / `{status, reason}` record.
pub(crate) const MODE_ALLSETTLED: u8 = 1;
/// `Promise.any` — the counter runs the other way. A fulfilment
/// settles the result at once, a rejection is collected, and the
/// count that has to reach zero is of REJECTIONS. The indexed slots
/// hold the `errors` list §27.2.4.2's AggregateError carries.
pub(crate) const MODE_ANY: u8 = 2;
/// `Promise.race` — the iterator-interleaved lane only (§27.2.4.5;
/// this fan-in never runs it). Nothing is collected and nothing
/// finishes: the first settlement wins, an empty iterable stays
/// pending, and the block exists to carry the outer promise the
/// minted resolver pair settles.
pub(crate) const MODE_RACE: u8 = 3;

/// State every element job of one fan-in shares.
#[repr(C)]
pub(crate) struct AllBlock {
    /// The outer promise. One stake, released when the last job goes.
    pub(crate) result: *mut c_void,
    /// The pre-sized result array — for `any`, the `errors` list.
    /// Owned here until it is handed to the outer (or to the
    /// AggregateError); NULL afterwards so the release path knows not
    /// to drop what it gave away.
    pub(crate) result_arr: *mut c_void,
    /// §27.2.4.1.3 remainingElementsCount — fulfilments still owed,
    /// or for `any` (§27.2.4.2.3) rejections still owed.
    pub(crate) remaining: u64,
    /// Jobs still holding this block. The block outlives the settlement
    /// because the losing jobs still have to run and release.
    pub(crate) jobs: u64,
    /// The form the slots hold. Comes from the call site when it could
    /// name one; otherwise from the first element to settle, which the
    /// typed tier guarantees describes all of them.
    pub(crate) elem_repr: u8,
    /// The outer has been settled — later jobs only release.
    pub(crate) done: u8,
    /// [`MODE_ALL`], [`MODE_ALLSETTLED`] or [`MODE_ANY`].
    pub(crate) mode: u8,
    /// allSettled only — the pair of class tags its records carry (see
    /// `combinator_allsettled::alloc_settled_struct`).
    pub(crate) record_tags: u64,
    /// allSettled only — the form each record's value slot must hold,
    /// so an element settled through the any lane is unboxed into it
    /// rather than parked as box bits. `elem_repr` above describes the
    /// RESULT array's slots, which for allSettled are always records.
    pub(crate) record_value_repr: u8,
    /// The input carried NaN-box slots, so its elements are reached
    /// through the any-slot walk rather than as raw promise pointers.
    /// This is about READING the input; what the result holds is
    /// `result_any`.
    pub(crate) input_any: u8,
    /// `all`'s result array holds NaN-box slots, because its elements
    /// have no single static form to share. Two independent reasons
    /// reach this, which is why it is not the same bit as `input_any`:
    /// the input was any-shape (so the elements were never described),
    /// or the CALL SITE typed the result element `any` — the
    /// heterogeneous `Promise.all([Promise<number>, Promise<string>])`,
    /// which the checker types `Promise<Array(Any)>` while every input
    /// slot is still a plain promise pointer. allSettled's result is a
    /// record array either way.
    pub(crate) result_any: u8,
}

#[repr(C)]
struct AllElemArg {
    block: *mut AllBlock,
    /// The element promise, owned by this job (see module doc).
    elem: *mut c_void,
    index: u64,
}

/// Release one job's hold on the block; free it (and anything it still
/// owns) when the last one lets go.
pub(crate) unsafe fn release_block(b: *mut AllBlock) {
    unsafe {
        (*b).jobs -= 1;
        if (*b).jobs != 0 {
            return;
        }
        // Non-NULL only when the outer rejected: the array never
        // reached it, so this is where it dies.
        if !(*b).result_arr.is_null() {
            __torajs_value_drop_heap((*b).result_arr);
        }
        __torajs_promise_drop((*b).result);
        free(b as *mut c_void);
    }
}

unsafe extern "C" fn all_elem_dispatch(arg: i64) {
    let a = arg as *mut AllElemArg;
    unsafe {
        let b = (*a).block;
        let ep = as_promise((*a).elem);
        if (*b).done == 0 {
            // `any` is the mirror: what short-circuits is a
            // FULFILMENT, and what the counter waits for is the last
            // rejection.
            let short_circuits = if (*b).mode == MODE_ANY {
                (*ep).state == STATE_FULFILLED
            } else {
                (*b).mode == MODE_ALL && (*ep).state == STATE_REJECTED
            };
            if short_circuits {
                crate::combinator_fanin_slot::settle_from(b, ep);
            } else {
                crate::combinator_fanin_slot::store_element(b, (*a).index, ep);
                (*b).remaining -= 1;
                if (*b).remaining == 0 {
                    crate::combinator_fanin_slot::finish(b);
                }
            }
        }
        __torajs_promise_drop((*a).elem);
        release_block(b);
        free(a as *mut c_void);
    }
}

/// Attach one job per element and answer the pending result.
///
/// Every element gets a job, settled or not: `attach_then` enqueues an
/// already-settled one immediately, so one path covers both and the
/// microtask order is the spec's rather than a mixture of synchronous
/// reads and jobs. A NULL slot has nothing to wait on and no value to
/// contribute, so it is filled in place and only counted out.
///
/// `target_repr` is the element form the call site named (0 when it
/// could not). Pre-stamping the result `REPR_HEAP` matters for the same
/// reason it does in `race`: an any-param attach can land before this
/// cell settles, and its gate refuses an UNSTAMPED source.
pub(crate) unsafe fn fan_in(
    promises_arr: *mut c_void,
    mode: u8,
    target_repr: u8,
    record_tags: u64,
    input_any: bool,
) -> *mut c_void {
    unsafe {
        let len = arr_len(promises_arr);
        if input_any {
            crate::combinator_any::absorb_any_inputs(promises_arr);
        } else {
            absorb_inputs(promises_arr);
        }
        let result = crate::pool::__torajs_promise_alloc_pending();
        if mode == MODE_ANY {
            // `any` forwards a single settled value rather than an
            // array, so its pre-stamp comes from the same place
            // `race`'s does — the first element's form, overwritten by
            // whichever element actually settles the outer.
            pre_stamp_from_first(result, promises_arr, len, input_any);
        } else {
            (*as_promise(result)).value_repr = REPR_HEAP;
            (*as_promise(result)).value_is_heap = 1;
        }

        // `any` fills an `errors` list; `all` answers an any-shape array
        // whenever its elements have no single static form to share (see
        // `AllBlock::result_any` for the two ways that happens);
        // everything else is a raw-slot array the jobs overwrite by
        // index.
        let result_any = mode == MODE_ALL && (input_any || target_repr == REPR_ANY);
        let result_arr = if mode == MODE_ANY {
            crate::combinator_aggregate::alloc_errors(len)
        } else if result_any {
            crate::combinator_any::alloc_any_result(len)
        } else {
            let mut a = __torajs_arr_alloc(len);
            for _ in 0..len {
                a = __torajs_arr_push(a, 0);
            }
            a
        };

        let b = malloc(core::mem::size_of::<AllBlock>()) as *mut AllBlock;
        (*b).result = result;
        (*b).result_arr = result_arr;
        (*b).remaining = len;
        // The setup itself counts as a holder, so a synchronous cascade
        // of already-settled elements cannot free the block from under
        // the loop still attaching to the rest.
        (*b).jobs = 1;
        // allSettled's slots always hold record cells, so their chain
        // is fixed; `all`'s comes from the call site (or the first
        // element to settle, when the site could not name one).
        (*b).elem_repr = if mode == MODE_ALLSETTLED {
            REPR_HEAP
        } else {
            target_repr
        };
        (*b).done = 0;
        (*b).mode = mode;
        (*b).record_tags = record_tags;
        (*b).record_value_repr = if mode == MODE_ALLSETTLED {
            target_repr
        } else {
            REPR_UNSTAMPED
        };
        (*b).input_any = u8::from(input_any);
        (*b).result_any = u8::from(result_any);
        __torajs_rc_inc(result);

        for i in 0..len {
            // `minted` marks a wrapper this loop made: its single
            // stake goes straight into the job, with no inc to pay.
            let (pp, minted) = if input_any {
                let bits = crate::combinator_any::any_slot(promises_arr, i);
                match crate::combinator_any::slot_promise(bits) {
                    Some(p) => (p, false),
                    // §27.2.4.2 gives a non-thenable element a
                    // `promiseResolve` wrapper like every other one,
                    // and for `any` that tick is observable (see
                    // `wrap_plain`).
                    None if mode == MODE_ANY => (wrap_plain(bits), true),
                    None => {
                        // §27.2.4.1 treats a non-thenable element as an
                        // already-fulfilled value. Nothing to wait on, so
                        // it lands now and only counts itself out.
                        crate::combinator_fanin_slot::store_plain(b, i, bits);
                        (*b).remaining -= 1;
                        continue;
                    }
                }
            } else {
                (arr_slot_ptr(promises_arr, i), false)
            };
            if pp.is_null() {
                (*b).remaining -= 1;
                continue;
            }
            // An element borrowed from the input array takes an inc to
            // pay for the stake its job consumes.
            if !minted {
                __torajs_rc_inc(pp as *mut c_void);
            }
            attach_elem_job(b, pp, i);
        }
        // An all-NULL (or empty) input owes nothing — §27.2.4.1 / .3
        // resolve with the empty array rather than waiting forever,
        // and §27.2.4.2 rejects with an empty AggregateError.
        if (*b).remaining == 0 && (*b).done == 0 {
            (*b).elem_repr = if (*b).elem_repr == REPR_UNSTAMPED && len == 0 {
                // Nothing will ever describe the slots of an empty
                // array; heap-chain it so the any lane can still read it.
                crate::layout::REPR_HEAP
            } else {
                (*b).elem_repr
            };
            crate::combinator_fanin_slot::finish(b);
        }
        release_block(b);
        result
    }
}

/// One element's job: take a block hold, park the (already-paid)
/// element stake in the arg, and attach the dispatcher — settled or
/// not, `attach_then` enqueues immediately for a settled source, so
/// one path covers both. Shared with the iterator-interleaved lane,
/// whose elements arrive one at a time rather than off an array.
pub(crate) unsafe fn attach_elem_job(b: *mut AllBlock, pp: *mut Promise, index: u64) {
    unsafe {
        let a = malloc(core::mem::size_of::<AllElemArg>()) as *mut AllElemArg;
        (*a).block = b;
        (*a).elem = pp as *mut c_void;
        (*a).index = index;
        (*b).jobs += 1;
        __torajs_promise_attach_then(pp as *mut c_void, Some(all_elem_dispatch), a as i64);
    }
}

/// §27.2.4.2's `promiseResolve` wrapper for a non-thenable element: an
/// already-fulfilled cell carrying the boxed value, so it reaches the
/// outer through a job like every other element.
///
/// `all` does not need one — a plain element only contributes a slot
/// there, and the result still waits for the rest. For `any` the plain
/// element DECIDES the result, so the tick it decides on is
/// observable: settling it during setup put the reaction a microtask
/// ahead of bun, since `.then` was attaching to an already-resolved
/// cell.
pub(crate) unsafe fn wrap_plain(bits: u64) -> *mut Promise {
    unsafe {
        // The wrapper owns what it carries; the input array keeps its
        // own stake.
        crate::combinator_any::box_share(bits);
        let p = crate::pool::__torajs_promise_alloc_fulfilled_heap(bits as i64);
        let pp = as_promise(p);
        (*pp).value_repr = crate::layout::REPR_ANY;
        pp
    }
}

/// Stamp the result cell from the first element that has a form, the
/// way `combinator::race_fan_in` does: an any-param attach can land
/// before this cell settles, and its gate refuses an UNSTAMPED source.
/// An any-shape input always settles boxed, so it needs no walk.
unsafe fn pre_stamp_from_first(
    result: *mut c_void,
    promises_arr: *mut c_void,
    len: u64,
    input_any: bool,
) {
    unsafe {
        let rp = as_promise(result);
        if input_any {
            (*rp).value_repr = crate::layout::REPR_ANY;
            (*rp).value_is_heap = 1;
            return;
        }
        // An empty input can never carry a value, so VOID keeps the
        // gate quiet — `race`'s note on why not UNSTAMPED applies here
        // verbatim.
        (*rp).value_repr = crate::layout::REPR_VOID;
        for i in 0..len {
            let pp = arr_slot_ptr(promises_arr, i);
            if !pp.is_null() {
                (*rp).value_repr = (*pp).value_repr;
                (*rp).value_is_heap = (*pp).value_is_heap;
                return;
            }
        }
    }
}

/// `Promise.all`'s fan-in. `target_repr` is the element form the call
/// site named (0 when it could not).
pub(crate) unsafe fn all_fan_in(promises_arr: *mut c_void, target_repr: u8) -> *mut c_void {
    unsafe { fan_in(promises_arr, MODE_ALL, target_repr, 0, false) }
}

/// `Promise.all`'s fan-in over an any-shape input.
pub(crate) unsafe fn all_fan_in_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe { fan_in(promises_arr, MODE_ALL, REPR_UNSTAMPED, 0, true) }
}

/// `Promise.any`'s fan-in. No element form is needed: a fulfilment
/// forwards the winner's own settlement and the `errors` list is an
/// `Array<Any>` either way.
pub(crate) unsafe fn any_fan_in(promises_arr: *mut c_void) -> *mut c_void {
    unsafe { fan_in(promises_arr, MODE_ANY, REPR_UNSTAMPED, 0, false) }
}

/// `Promise.any`'s fan-in over an any-shape input.
pub(crate) unsafe fn any_fan_in_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe { fan_in(promises_arr, MODE_ANY, REPR_UNSTAMPED, 0, true) }
}

/// `Promise.allSettled`'s fan-in over an any-shape input. The records
/// hold boxed values, so the value form is `REPR_ANY` — nothing to
/// unbox into.
pub(crate) unsafe fn allsettled_fan_in_any(
    promises_arr: *mut c_void,
    record_tags: u64,
) -> *mut c_void {
    unsafe {
        fan_in(
            promises_arr,
            MODE_ALLSETTLED,
            crate::layout::REPR_ANY,
            record_tags,
            true,
        )
    }
}

/// `Promise.allSettled`'s fan-in. `record_tags` is the class-tag pair
/// its records carry; the slots are record cells, so no element form is
/// needed.
pub(crate) unsafe fn allsettled_fan_in(
    promises_arr: *mut c_void,
    record_tags: u64,
    value_repr: u8,
) -> *mut c_void {
    unsafe {
        fan_in(
            promises_arr,
            MODE_ALLSETTLED,
            value_repr,
            record_tags,
            false,
        )
    }
}

/// True when some element of an any-shape input has not settled yet.
/// Plain (non-promise) slots are already-fulfilled values, so they
/// never hold the result up.
pub(crate) unsafe fn has_pending_any(promises_arr: *mut c_void) -> bool {
    unsafe {
        let len = arr_len(promises_arr);
        for i in 0..len {
            let bits = crate::combinator_any::any_slot(promises_arr, i);
            if let Some(pp) = crate::combinator_any::slot_promise(bits)
                && (*pp).state != STATE_FULFILLED
                && (*pp).state != STATE_REJECTED
            {
                return true;
            }
        }
        false
    }
}

/// True when some element has not settled yet — the only case the sync
/// kernels cannot answer on their own.
pub(crate) unsafe fn has_pending(promises_arr: *mut c_void) -> bool {
    unsafe {
        let len = arr_len(promises_arr);
        for i in 0..len {
            let pp = arr_slot_ptr(promises_arr, i);
            if !pp.is_null() && (*pp).state != STATE_FULFILLED && (*pp).state != STATE_REJECTED {
                return true;
            }
        }
        false
    }
}
