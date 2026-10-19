//! An async function frame, as the trial-deletion walk sees it.
//!
//! Mirror of torajs-promise `frame.rs` (the offsets below are checked
//! against its struct where both crates are linked, in torajs-value-
//! drop's frame tests):
//!
//! ```text
//!   +0   universal heap header (type_tag = TAG_ASYNC_FRAME)
//!   +8   desc      { resume, drop, trace }, each relative to the
//!                  desc; holds the block size once
//!                  a buffered frame's drop parked it for the collector
//!   +24  direct    FRAME_COLLECTED once the collector frees the frame
//!   +32  result    the async function's promise (one reference)
//!   +40  next      link in the callback list of the promise it waits on
//!   +48  awaiting  that promise: a reference, or, with bit 0 set while
//!                  the frame is parked, a back-pointer
//! ```
//!
//! `next` and `awaiting` sit where an inline derived promise keeps its
//! `rx_next` and `rx_source`, so the promise walk unlinks either kind
//! of list entry with the same two word offsets (torajs-cycle
//! `promise`).
//!
//! A frame is walked like a closure env, through its synthesized
//! `trace` (closure trace ABI): it reports `result`, an untagged
//! `awaiting` and the slots live at the current suspension point,
//! each with the address of the word holding it, so clearing replays
//! it. Its teardown belongs to the synthesized `drop`, told by
//! `direct` that the collector frees it: it then releases what the
//! trace reports and the collect left, instead of re-entering the body. A parked frame
//! is itself a child of the promise it waits on (the list holds its
//! reference; torajs-cycle `promise`).
//!
//! Everything here is a tag test or a word read: a program that never
//! makes a frame pays for no frame code in its collector.

use core::ffi::c_void;

use crate::closure_walk::TraceFn;

/// `torajs_rc::Tag::AsyncFrame`.
pub const TAG_ASYNC_FRAME: u16 = 32;
/// `AsyncFrame::desc`.
pub const FRAME_DESC_OFF: usize = 8;
/// `AsyncFrame::direct`.
pub const FRAME_DIRECT_OFF: usize = 24;
/// torajs-promise `frame::FRAME_COLLECTED`.
pub const FRAME_COLLECTED: i64 = 2;
/// `AsyncFrame::next`.
pub const FRAME_NEXT_OFF: usize = 40;
/// `AsyncFrame::awaiting`.
pub const FRAME_AWAITING_OFF: usize = 48;
/// `FrameDesc::drop`.
pub const DESC_DROP_OFF: usize = 8;
/// `FrameDesc::trace`.
pub const DESC_TRACE_OFF: usize = 16;

/// A desc row and the desc's address: each row holds its function's
/// address relative to the desc (torajs-promise `FrameDesc`), 0 when
/// there is none.
#[inline(always)]
unsafe fn desc_row(p: *mut c_void, off: usize) -> (i64, i64) {
    unsafe {
        let desc = *((p as *const u8).add(FRAME_DESC_OFF) as *const *const u8);
        (*(desc.add(off) as *const i64), desc as i64)
    }
}

/// The frame's trace.
///
/// # Safety
/// `p` is a live frame.
#[inline(always)]
pub(crate) unsafe fn frame_trace_fn(p: *mut c_void) -> Option<TraceFn> {
    let (rel, base) = unsafe { desc_row(p, DESC_TRACE_OFF) };
    if rel == 0 {
        None
    } else {
        let addr = base.wrapping_add(rel) as usize;
        Some(unsafe { core::mem::transmute::<usize, TraceFn>(addr) })
    }
}

/// Run a corpse frame's synthesized drop, marked collected: it
/// releases what the collect left in its slots and frees the block.
///
/// # Safety
/// `p` is a swept frame corpse at rc 0.
#[inline(always)]
pub(crate) unsafe fn frame_corpse_drop(p: *mut c_void) {
    unsafe {
        *((p as *mut u8).add(FRAME_DIRECT_OFF) as *mut i64) = FRAME_COLLECTED;
        let (rel, base) = desc_row(p, DESC_DROP_OFF);
        let addr = base.wrapping_add(rel) as usize;
        core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void)>(addr)(p);
    }
}

/// The block size a buffered frame's drop recorded over `desc` when
/// it left the free to the collector. That drop also cleared the
/// colour bits: the root walk frees a parked cell only when it reads
/// black at rc 0, and takes any other colour for a release still in
/// progress.
///
/// # Safety
/// `p` is such a parked frame.
#[inline(always)]
pub(crate) unsafe fn parked_frame_size(p: *mut c_void) -> usize {
    unsafe { *((p as *const u8).add(FRAME_DESC_OFF) as *const usize) }
}

#[cfg(test)]
mod tests;
