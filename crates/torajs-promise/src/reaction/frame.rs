//! The settlement of a derived cell whose sole waiter is a parked async
//! frame (`crate::frame`), for the dispatcher of [`super`].

use core::ffi::c_void;
use core::ptr;

use super::{__torajs_microtask_enqueue, Stake, queue_idle};
use crate::frame::{AsyncFrame, desc_resume};
use crate::layout::{
    Promise, PromiseCb, REPR_ANY, REPR_HEAP, REPR_STR, STATE_FULFILLED, STATE_PENDING, frame_cell,
};
use crate::then_box::{reject_on_pending_throw, settle_handler_return};

/// Resume the frame a settlement handed the dispatcher's stake to.
///
/// # Safety
/// `f` is a live frame whose `awaiting` holds that stake.
#[inline(always)]
pub(super) unsafe fn resume(f: *mut AsyncFrame) {
    unsafe { desc_resume((*f).desc)(f as i64) }
}

/// [`super::settle_into_chain`] for a list whose first link is a parked async
/// frame: when it is the only waiter and the return is a form that can
/// be neither adopted nor a thenable, fulfil `dp` here and hand the
/// dispatcher's stake on `dp` to the frame as its `awaiting` (the one
/// reference a queued frame holds on what it waited for, released by
/// its landing). The frame then resumes next in this job when its
/// resumption would be the very next job (§27.7.5.3 Await's reaction,
/// queued behind nothing), or is queued. Anything else keeps the
/// general settle, which queues every waiter in registration order.
#[inline(never)]
pub(super) unsafe fn settle(
    dp: *mut Promise,
    link: *mut PromiseCb,
    ret_repr: u8,
    value: i64,
) -> Stake {
    unsafe {
        let frame = frame_cell(link);
        if !(*frame).next.is_null()
            || matches!(ret_repr, REPR_HEAP | REPR_ANY)
            || (*dp).state != STATE_PENDING
        {
            settle_handler_return(dp as *mut c_void, ret_repr, value);
            return Stake::Kept;
        }
        if reject_on_pending_throw(dp as *mut c_void) {
            return Stake::Kept;
        }
        (*dp).value_repr = ret_repr;
        (*dp).value_is_heap = (ret_repr == REPR_STR) as u8;
        (*dp).state = STATE_FULFILLED;
        (*dp).value = value;
        (*dp).callbacks = ptr::null_mut();
        (*frame).awaiting = dp as *mut c_void;
        if queue_idle() {
            Stake::RunFrame(frame)
        } else {
            __torajs_microtask_enqueue(desc_resume((*frame).desc), frame as i64);
            Stake::Queued
        }
    }
}
