//! Async function frames: the heap cell a suspended async function
//! keeps its state in, and the runtime half of its suspension.
//!
//! ```text
//!   +0   universal heap header (type_tag = TAG_ASYNC_FRAME)
//!   +8   desc      *const FrameDesc, the function's static row
//!   +16  state     0 running / not started, k >= 1 parked at the
//!                  k-th suspension point, FRAME_STATE_DONE after
//!   +24  direct    the resumption value of an await on a value that
//!                  is not a thenable
//!   +32  result    the async function's promise; the frame holds +1
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
use crate::reaction::record::TraceVisit;
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

/// The frame's trace, in the closure trace ABI: it reports `result`,
/// `awaiting` when it is owned (bit [`AWAITING_PARKED`] clear), the
/// parameter words the frame holds, and, while the frame is parked,
/// the words its suspension point keeps (what the body owns there),
/// each with the address of the word holding it. The cycle collector
/// walks and clears a frame through it alone.
pub type FrameTrace = unsafe extern "C" fn(*mut c_void, TraceVisit, *mut c_void);

/// One per async function, in read-only data next to the code. Each
/// row holds its function's address relative to the desc (the
/// relative vtable shape: both are link-time constants of one image,
/// so the slide cancels and the table needs no load-time fixup); read
/// them through [`desc_resume`] and [`desc_drop`] (torajs-cycle reads
/// `trace` the same way).
#[repr(C)]
pub struct FrameDesc {
    /// Re-enters the body at the point `state` names.
    pub resume: i64,
    /// Releases what the frame owns at its `state` and frees it; runs
    /// once the last reference is gone, with no rc gate of its own.
    /// A frame left at a suspension point is first unwound through its
    /// body (`body`: [`__torajs_frame_unwind`]).
    /// It releases `result` and an owned `awaiting` through
    /// [`__torajs_frame_release_links`]. A frame the cycle collector
    /// holds as a candidate root (`FLAG_BUFFERED` in its header) is
    /// not freed: the drop writes the block size over `desc` (+8),
    /// clears the colour bits (black, as the collector frees only a
    /// black cell at rc 0) and returns, and the collector frees the
    /// block later by that size. That is the closure env's ending,
    /// `emit_env_free_or_defer` in torajs-core, with the size word's
    /// offset made a parameter.
    pub drop: i64,
    /// Every frame has one, never 0: its `result` is always an edge.
    pub trace: i64,
}

impl FrameDesc {
    /// Write the three rows of the desc at `row` for the given
    /// functions, each relative to `row`, as the link lays out a
    /// compiled one.
    ///
    /// # Safety
    /// `row` is writable and stays put while frames use it.
    pub unsafe fn fill(
        row: *mut FrameDesc,
        resume: MicrotaskFn,
        drop: unsafe extern "C" fn(*mut c_void),
        trace: FrameTrace,
    ) {
        let base = row as i64;
        unsafe {
            (*row).resume = resume as usize as i64 - base;
            (*row).drop = drop as usize as i64 - base;
            (*row).trace = trace as usize as i64 - base;
        }
    }
}

#[inline(always)]
unsafe fn desc_addr(desc: *const FrameDesc, rel: i64) -> usize {
    (desc as i64).wrapping_add(rel) as usize
}

/// The resume function `desc` names.
///
/// # Safety
/// `desc` is a frame descriptor.
#[inline(always)]
pub unsafe fn desc_resume(desc: *const FrameDesc) -> MicrotaskFn {
    unsafe { core::mem::transmute::<usize, MicrotaskFn>(desc_addr(desc, (*desc).resume)) }
}

/// The drop function `desc` names.
///
/// # Safety
/// `desc` is a frame descriptor.
#[inline(always)]
pub unsafe fn desc_drop(desc: *const FrameDesc) -> unsafe extern "C" fn(*mut c_void) {
    unsafe {
        core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void)>(desc_addr(
            desc,
            (*desc).drop,
        ))
    }
}

/// The trace function `desc` names.
///
/// # Safety
/// `desc` is a frame descriptor.
#[inline(always)]
pub unsafe fn desc_trace(desc: *const FrameDesc) -> FrameTrace {
    unsafe { core::mem::transmute::<usize, FrameTrace>(desc_addr(desc, (*desc).trace)) }
}

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
pub use body::{__torajs_frame_unwind, FRAME_CANCELLED, FRAME_COLLECTED, TRACE_STATE_WORD};

#[cfg(test)]
mod tests;
