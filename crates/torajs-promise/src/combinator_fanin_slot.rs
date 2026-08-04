//! What one element of a fan-in contributes, and how the result is
//! finally settled.
//!
//! Split verbatim out of [`crate::combinator_all_fanin`], which held
//! both this and the block's own lifecycle and had run out of room
//! before `Promise.any` — the fourth combinator — could be written into
//! it. The line is between the two halves that were already there: the
//! block and its jobs live next door, and everything here answers "what
//! does this element put in its slot, and what does the outer promise
//! finally get".

use core::ffi::c_void;

use crate::combinator::{repr_arr_kind_chain, unbox_target};
use crate::combinator_all_fanin::{AllBlock, MODE_ALL, MODE_ALLSETTLED};
use crate::layout::{Promise, REPR_HEAP, REPR_UNSTAMPED, STATE_FULFILLED, as_promise};
use crate::state::{__torajs_promise_reject, __torajs_promise_resolve};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// Write logical slot `i` of a raw-slot array. Every slot was pushed
/// before any job could run, so nothing grows underneath this; the data
/// pointer is re-read each time regardless, since it lives in the cell.
pub(crate) unsafe fn store_slot(arr: *mut c_void, i: u64, v: i64) {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(crate::layout::ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(bytes.add(crate::layout::ARR_DATA_PTR_OFF) as *const *mut u8);
        *(data.add(((head + i) * 8) as usize) as *mut i64) = v;
    }
}

/// One element reported in: park what it contributes in the slot it was
/// given.
pub(crate) unsafe fn store_element(b: *mut AllBlock, index: u64, ep: *mut Promise) {
    unsafe {
        if (*b).mode == MODE_ALLSETTLED {
            let (v, owned) = crate::combinator_allsettled::record_slot(
                (*ep).value_repr,
                (*b).record_value_repr,
                (*ep).value,
            );
            let rec = crate::combinator_allsettled::alloc_settled_struct(
                (*ep).state,
                v,
                (*b).record_tags,
            );
            // The record co-owns a heap-typed inner value, exactly as
            // the synchronous allSettled kernel's inc does.
            if owned && v != 0 {
                __torajs_rc_inc(v as *mut c_void);
            }
            store_slot((*b).result_arr, index, rec as i64);
            return;
        }
        if (*b).input_any != 0 && (*b).mode == MODE_ALL {
            // The result is an `Array<Any>`; its slots hold boxes, and
            // the array keeps its own stake on each (`box_settled_owned`
            // incs, NaN-box aware so immediates pass through). An
            // UNSTAMPED element has no form to box from — the sync
            // sibling refuses those up front, and undefined keeps this
            // total without a panic path.
            let boxed = crate::combinator_any::box_settled_owned((*ep).value_repr, (*ep).value)
                .unwrap_or_else(|| crate::combinator_any::box_undefined());
            store_slot((*b).result_arr, index, boxed as i64);
            return;
        }
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
pub(crate) unsafe fn fulfil(b: *mut AllBlock) {
    unsafe {
        let arr = (*b).result_arr;
        let rp = as_promise((*b).result);
        // Only `all` over an any-shape input answers an any-shape array;
        // allSettled's result is a raw-slot array of record cells either
        // way, and it still needs its chain mark — the same condition
        // the allocation above is chosen by. Getting this wrong left the
        // records unmarked and the any lane read them as nulls.
        if (*b).input_any != 0 && (*b).mode == MODE_ALL {
            // An `Array<Any>` describes its own slots; there is no
            // elem-kind chain to mark.
            (*rp).value_is_heap = 1;
            (*rp).value_repr = REPR_HEAP;
            (*b).result_arr = core::ptr::null_mut();
            (*b).done = 1;
            __torajs_promise_resolve((*b).result, arr as i64);
            return;
        }
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
pub(crate) unsafe fn reject_from(b: *mut AllBlock, ep: *mut Promise) {
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

/// A plain (non-promise) element of an any-shape input: already
/// fulfilled, so it contributes at setup rather than through a job.
pub(crate) unsafe fn store_plain(b: *mut AllBlock, index: u64, bits: u64) {
    unsafe {
        if (*b).mode == MODE_ALLSETTLED {
            let rec = crate::combinator_allsettled::alloc_settled_struct(
                STATE_FULFILLED,
                bits as i64,
                (*b).record_tags,
            );
            crate::combinator_any::box_share(bits);
            store_slot((*b).result_arr, index, rec as i64);
            return;
        }
        crate::combinator_any::box_share(bits);
        store_slot((*b).result_arr, index, bits as i64);
    }
}
