//! Real fan-in for `Promise.all` (§27.2.4.1) — the path taken when an
//! element has not settled yet.
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

use core::ffi::c_void;

use crate::combinator::{absorb_inputs, arr_len, arr_slot_ptr, repr_arr_kind_chain, unbox_target};
use crate::layout::{
    Promise, REPR_HEAP, REPR_UNSTAMPED, STATE_FULFILLED, STATE_REJECTED, as_promise,
};
use crate::pool::__torajs_promise_drop;
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
};

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

/// State every element job of one `Promise.all` call shares.
#[repr(C)]
struct AllBlock {
    /// The outer promise. One stake, released when the last job goes.
    result: *mut c_void,
    /// The pre-sized result array. Owned here until it is handed to the
    /// outer on fulfilment; NULL afterwards so the release path knows
    /// not to drop what it gave away.
    result_arr: *mut c_void,
    /// §27.2.4.1.3 remainingElementsCount — fulfilments still owed.
    remaining: u64,
    /// Jobs still holding this block. The block outlives the settlement
    /// because the losing jobs still have to run and release.
    jobs: u64,
    /// The form the slots hold. Comes from the call site when it could
    /// name one; otherwise from the first element to settle, which the
    /// typed tier guarantees describes all of them.
    elem_repr: u8,
    /// The outer has been settled — later jobs only release.
    done: u8,
}

#[repr(C)]
struct AllElemArg {
    block: *mut AllBlock,
    /// The element promise, owned by this job (see module doc).
    elem: *mut c_void,
    index: u64,
}

/// Write logical slot `i` of a raw-slot array. Every slot was pushed
/// before any job could run, so nothing grows underneath this; the data
/// pointer is re-read each time regardless, since it lives in the cell.
unsafe fn store_slot(arr: *mut c_void, i: u64, v: i64) {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(crate::layout::ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(bytes.add(crate::layout::ARR_DATA_PTR_OFF) as *const *mut u8);
        *(data.add(((head + i) * 8) as usize) as *mut i64) = v;
    }
}

/// Release one job's hold on the block; free it (and anything it still
/// owns) when the last one lets go.
unsafe fn release_block(b: *mut AllBlock) {
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

/// One element fulfilled: park its value in the slot it was given.
unsafe fn store_element(b: *mut AllBlock, index: u64, ep: *mut Promise) {
    unsafe {
        if (*b).elem_repr == REPR_UNSTAMPED {
            (*b).elem_repr = (*ep).value_repr;
        }
        let v = match unbox_target((*ep).value_repr, (*b).elem_repr) {
            Some(lane) => crate::then_box::unbox_settled(lane, (*ep).value),
            None => (*ep).value,
        };
        // Same payment the sync kernel makes: a heap-chained result
        // array drops every slot it holds, while the element promise
        // keeps its own stake.
        if repr_arr_kind_chain((*b).elem_repr) == Some(4) && v != 0 {
            __torajs_rc_inc(v as *mut c_void);
        }
        store_slot((*b).result_arr, index, v);
    }
}

/// Every element reported in — hand the array to the outer.
unsafe fn fulfil(b: *mut AllBlock) {
    unsafe {
        let arr = (*b).result_arr;
        let rp = as_promise((*b).result);
        let repr = match repr_arr_kind_chain((*b).elem_repr) {
            Some(chain) => {
                __torajs_arr_mark_kind(arr, chain);
                REPR_HEAP
            }
            // Unmarkable slots — keep the settled cell loud rather than
            // hand the any lane a misdecoding array (the sync kernel's
            // posture, verbatim).
            None => REPR_UNSTAMPED,
        };
        (*rp).value_is_heap = 1;
        (*rp).value_repr = repr;
        // The array's stake moves to the promise; the block must not
        // drop it when the last job leaves.
        (*b).result_arr = core::ptr::null_mut();
        (*b).done = 1;
        __torajs_promise_resolve((*b).result, arr as i64);
    }
}

/// §27.2.4.1.2 — the first rejection settles the outer with its reason.
unsafe fn reject_from(b: *mut AllBlock, ep: *mut Promise) {
    unsafe {
        let value = (*ep).value;
        let is_heap = (*ep).value_is_heap;
        let rp = as_promise((*b).result);
        (*rp).value_is_heap = is_heap;
        (*rp).value_repr = (*ep).value_repr;
        if is_heap != 0 && value != 0 {
            __torajs_rc_inc(value as *mut c_void);
        }
        (*b).done = 1;
        __torajs_promise_reject((*b).result, value);
    }
}

unsafe extern "C" fn all_elem_dispatch(arg: i64) {
    let a = arg as *mut AllElemArg;
    unsafe {
        let b = (*a).block;
        let ep = as_promise((*a).elem);
        if (*b).done == 0 {
            if (*ep).state == STATE_REJECTED {
                reject_from(b, ep);
            } else {
                store_element(b, (*a).index, ep);
                (*b).remaining -= 1;
                if (*b).remaining == 0 {
                    fulfil(b);
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
pub(crate) unsafe fn all_fan_in(promises_arr: *mut c_void, target_repr: u8) -> *mut c_void {
    unsafe {
        let len = arr_len(promises_arr);
        absorb_inputs(promises_arr);
        let result = crate::pool::__torajs_promise_alloc_pending();
        (*as_promise(result)).value_repr = REPR_HEAP;
        (*as_promise(result)).value_is_heap = 1;

        let mut result_arr = __torajs_arr_alloc(len);
        for _ in 0..len {
            result_arr = __torajs_arr_push(result_arr, 0);
        }

        let b = malloc(core::mem::size_of::<AllBlock>()) as *mut AllBlock;
        (*b).result = result;
        (*b).result_arr = result_arr;
        (*b).remaining = len;
        // The setup itself counts as a holder, so a synchronous cascade
        // of already-settled elements cannot free the block from under
        // the loop still attaching to the rest.
        (*b).jobs = 1;
        (*b).elem_repr = target_repr;
        (*b).done = 0;
        __torajs_rc_inc(result);

        for i in 0..len {
            let pp = arr_slot_ptr(promises_arr, i);
            if pp.is_null() {
                (*b).remaining -= 1;
                continue;
            }
            let a = malloc(core::mem::size_of::<AllElemArg>()) as *mut AllElemArg;
            (*a).block = b;
            (*a).elem = pp as *mut c_void;
            (*a).index = i;
            (*b).jobs += 1;
            // The element is borrowed from the input array, so it takes
            // an inc to pay for the stake its job consumes.
            __torajs_rc_inc(pp as *mut c_void);
            __torajs_promise_attach_then(pp as *mut c_void, Some(all_elem_dispatch), a as i64);
        }
        // An all-NULL (or empty) input owes nothing — §27.2.4.1 resolves
        // with the empty array rather than waiting forever.
        if (*b).remaining == 0 && (*b).done == 0 {
            (*b).elem_repr = if (*b).elem_repr == REPR_UNSTAMPED && len == 0 {
                // Nothing will ever describe the slots of an empty
                // array; heap-chain it so the any lane can still read it.
                crate::layout::REPR_HEAP
            } else {
                (*b).elem_repr
            };
            fulfil(b);
        }
        release_block(b);
        result
    }
}

/// True when some element has not settled yet — the only case the sync
/// kernel cannot answer on its own.
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
