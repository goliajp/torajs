//! Async function frames: the heap cell a suspended async function
//! keeps its state in, and the runtime half of its suspension.
//!
//! ```text
//!   +0   universal heap header (type_tag = TAG_ASYNC_FRAME)
//!   +8   desc      *const FrameDesc, the function's static row
//!   +16  state     0 before the first suspension, k >= 1 parked at
//!                  the k-th suspension point (or running since it
//!                  landed there), FRAME_STATE_DONE after
//!   +24  direct    the frame's teardown state: 0 while its body may
//!                  still run, FRAME_CANCELLED while its drop unwinds
//!                  the body, FRAME_COLLECTED while the cycle
//!                  collector frees it (`body`)
//!   +32  result    the async function's promise; the frame holds +1
//!                  (NULL for the module body's waiter, `module_await`)
//!   +40  next      this frame's link in its source's callback list
//!   +48  awaiting  the promise it waits on (see Ownership)
//!   +56  params and the values live across a suspension point
//! ```
//!
//! The frame is a callback-list member itself: awaiting a pending
//! promise parks the frame in that promise's list under a
//! `layout::CB_FRAME_TAG` link, with no reaction record and no derived
//! promise. Settlement queues `(desc.resume, frame)` like any other
//! reaction (§27.2.1.8). `next` and `awaiting` are at the offsets of
//! an inline derived promise's `rx_next` and `rx_source`, the link and
//! the back-pointer of the other list member that is a cell, so the
//! cycle collector unlinks both kinds through the same two words.
//!
//! ## Ownership
//!
//! - The frame has exactly one owner at a time: the running resume
//!   (or the function that made it), then the source's callback list
//!   while parked, then the microtask queue while queued. Parking and
//!   queueing hand that reference over; they never take a new one.
//!   So a frame's rc is 1 for as long as it lives, and its body is
//!   the only thing that reaches it while it runs: no release of it
//!   leaves it in the cycle collector's buffer, and no cell has an
//!   edge to it (a callback list holds it only while it is parked).
//!   The trace relies on this. While the body runs, `state` still
//!   names the point it landed from, whose words the body has taken
//!   back, so a trace of a running frame would report words it no
//!   longer owns; none ever runs (`body::running` checks it in debug
//!   builds at every call the body makes on its frame).
//! - `awaiting` follows the inline reaction's rule (`reaction.rs`):
//!   while parked it is a back-pointer, tagged with
//!   [`AWAITING_PARKED`], and the source clears it when it dies; once
//!   queued it is one reference, released by the landing. So a
//!   non-null untagged `awaiting` is always owned, whatever reads it
//!   (the frame's drop, the cycle collector's walk).
//! - `result` is one reference, released by the frame's drop. The two
//!   form a ring when the function awaits its own result (the frame
//!   parks in the result's list, which holds it): the trace reports
//!   `result`, the promise walk reports the parked frame, and the
//!   cycle collector frees both.
//! - A frame or a result this module lets go of is released through
//!   the universal drop, never by a direct call: the release of a
//!   discarded chain of frames awaiting one another nests once per
//!   frame, and that entry defers it once the stack runs deep.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{
    HeapHeader, MicrotaskFn, Promise, PromiseCb, REPR_ANY, REPR_HEAP, REPR_STR, STATE_PENDING,
    as_promise, frame_link,
};
use crate::pool::__torajs_promise_drop;
use torajs_rc::{rc_dec_cell, rc_inc_cell};

unsafe extern "C" {
    fn __torajs_microtask_enqueue(fn_: MicrotaskFn, arg: i64);
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `torajs_rc::Tag::AsyncFrame`.
pub const TAG_ASYNC_FRAME: u16 = 32;

// torajs-cycle `frame` and torajs-value-drop read these words at
// fixed offsets
const _: () = {
    assert!(TAG_ASYNC_FRAME == torajs_rc::Tag::AsyncFrame as u16);
    assert!(core::mem::offset_of!(AsyncFrame, desc) == 8);
    assert!(core::mem::offset_of!(AsyncFrame, result) == 32);
    assert!(core::mem::offset_of!(AsyncFrame, next) == core::mem::offset_of!(Promise, rx_next));
    assert!(
        core::mem::offset_of!(AsyncFrame, awaiting) == core::mem::offset_of!(Promise, rx_source)
    );
    assert!(core::mem::size_of::<AsyncFrame>() == FRAME_HEADER_SIZE);
    assert!(core::mem::offset_of!(FrameDesc, drop) == 8);
    assert!(core::mem::offset_of!(FrameDesc, trace) == 16);
};

/// `AsyncFrame::state` once the body has settled its result.
pub const FRAME_STATE_DONE: i64 = -1;

/// Bit 0 of `AsyncFrame::awaiting` while the frame is parked: the
/// word is then a back-pointer, not a reference.
pub const AWAITING_PARKED: usize = 1;

#[repr(C)]
pub struct AsyncFrame {
    pub header: HeapHeader,
    pub desc: *const FrameDesc,
    pub state: i64,
    pub direct: i64,
    pub result: *mut c_void,
    pub next: *mut PromiseCb,
    pub awaiting: *mut c_void,
}

/// Bytes before the parameter area.
pub const FRAME_HEADER_SIZE: usize = 56;

/// The promise `frame` holds a reference on through `awaiting`, or
/// NULL while it is parked or waits on nothing.
///
/// # Safety
/// `frame` is a live frame.
#[inline]
pub unsafe fn awaiting_owned(frame: *const AsyncFrame) -> *mut c_void {
    let a = unsafe { (*frame).awaiting };
    if a as usize & AWAITING_PARKED != 0 {
        ptr::null_mut()
    } else {
        a
    }
}

/// The async function's result promise, pending, its value form
/// stamped from the body's return type: `repr` is a `REPR_*` code.
///
/// # Safety
/// None beyond the allocator's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_result_alloc(repr: i64) -> *mut c_void {
    let p = unsafe { crate::pool::alloc_pending() };
    let pp = as_promise(p);
    let repr = repr as u8;
    unsafe {
        (*pp).value_repr = repr;
        (*pp).value_is_heap = matches!(repr, REPR_STR | REPR_HEAP | REPR_ANY) as u8;
    }
    p
}

/// §27.7.5.3 Await, steps 7-10, on a native promise `src` the caller
/// hands over one reference to. A settled `src` queues the resume at
/// once and that reference becomes the job's; a pending one parks the
/// frame in its list and the reference is released. Either way the
/// frame's own reference goes with it, and the caller must not touch
/// the frame again: a released `src` can die here and take the frame.
///
/// # Safety
/// `frame` is a live frame its caller owns, not parked or queued;
/// `src` is a live promise cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_await_promise(frame: *mut c_void, src: *mut c_void) {
    let f = frame as *mut AsyncFrame;
    body::running(f);
    let sp = as_promise(src);
    unsafe {
        // PerformPromiseThen step 12: the source is handled from here on
        (*sp).has_handler = 1;
        if (*sp).state != STATE_PENDING {
            (*f).awaiting = src;
            __torajs_microtask_enqueue(desc_resume((*f).desc), frame as i64);
            return;
        }
        (*f).awaiting = (src as usize | AWAITING_PARKED) as *mut c_void;
        (*f).next = (*sp).callbacks;
        (*sp).callbacks = frame_link(f);
        // the frame's slots can hold anything: the tree under `src`
        // now has an edge out
        crate::reaction::candidate::mark_list_edge(sp);
        __torajs_promise_drop(src);
    }
}

/// Release one reference on `frame`: the last one runs its drop, any
/// other leaves it to the cycle collector's candidate test.
///
/// # Safety
/// `frame` is a live frame and the caller gives up a reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_drop(frame: *mut c_void) {
    unsafe { release(frame as *mut AsyncFrame) }
}

#[inline(always)]
unsafe fn release(frame: *mut AsyncFrame) {
    unsafe {
        if rc_dec_cell(frame as *mut c_void) {
            desc_drop((*frame).desc)(frame as *mut c_void);
        } else {
            __torajs_cycle_buffer(frame as *mut c_void);
        }
    }
}

/// The part of a frame's drop that does not depend on its state:
/// release `result` and an owned `awaiting`, and clear both. Either
/// may already be NULL (the cycle collector clears the edges it
/// accounted for).
///
/// # Safety
/// `frame` is a frame whose last reference is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_release_links(frame: *mut c_void) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        let src = awaiting_owned(f);
        (*f).awaiting = ptr::null_mut();
        let result = (*f).result;
        (*f).result = ptr::null_mut();
        // the result's death releases the frames parked in its list
        __torajs_value_drop_heap(src);
        __torajs_value_drop_heap(result);
    }
}

/// `source` settled with `frame` parked in its list: the job takes a
/// reference on `source`, and the list's reference on the frame
/// passes to the queue. The caller has read the frame's `next`
/// already. Out of line, and with nothing to answer, so the drain of a
/// list without frames keeps its shape and its registers.
#[inline(never)]
pub(crate) unsafe fn queue_frame(source: *mut Promise, frame: *mut AsyncFrame) {
    unsafe {
        (*frame).next = ptr::null_mut();
        (*frame).awaiting = source as *mut c_void;
        rc_inc_cell(source as *mut c_void);
        __torajs_microtask_enqueue(desc_resume((*frame).desc), frame as i64);
    }
}

/// The promise `frame` was parked on is dying (`frame` already out of
/// its list): cut the back-pointer and let go of the list's
/// reference. The frame's result stays pending for good, as it would
/// in any engine once nothing can settle what it waits on.
#[inline(never)]
pub(crate) unsafe fn source_died(frame: *mut AsyncFrame) {
    unsafe {
        (*frame).next = ptr::null_mut();
        (*frame).awaiting = ptr::null_mut();
        __torajs_value_drop_heap(frame as *mut c_void);
    }
}

mod body;
mod desc;
pub use body::{
    __torajs_frame_trace_links, __torajs_frame_unwind, FRAME_CANCELLED, FRAME_COLLECTED,
    TRACE_STATE_WORD,
};
pub use desc::{FrameDesc, FrameTrace, desc_drop, desc_resume, desc_trace};

#[cfg(test)]
mod tests;
