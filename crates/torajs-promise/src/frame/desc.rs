//! The frame descriptor: one static row per async function, naming
//! its resume, drop and trace, and the readers of those rows.

use core::ffi::c_void;

use crate::layout::MicrotaskFn;
use crate::reaction::record::TraceVisit;

/// The frame's trace, in the closure trace ABI: it reports `result`,
/// `awaiting` when it is owned (bit
/// [`AWAITING_PARKED`](super::AWAITING_PARKED) clear), the
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
    /// body (`body`:
    /// [`__torajs_frame_unwind`](super::__torajs_frame_unwind)).
    /// It releases `result` and an owned `awaiting` through
    /// [`__torajs_frame_release_links`](super::__torajs_frame_release_links).
    /// A frame the cycle collector holds as a candidate root
    /// (`FLAG_BUFFERED` in its header) is not freed: the drop writes
    /// the block size over `desc` (+8), clears the colour bits (black,
    /// as the collector frees only a black cell at rc 0) and returns,
    /// and the collector frees the block later by that size. That is the closure env's ending,
    /// `emit_env_free_or_defer` in torajs-core, with the size word's
    /// offset made a parameter. The frame's rc stays 1 for its whole
    /// life (see the ownership notes in `frame`), so no release of a
    /// frame leaves it buffered and this branch is not taken today; it
    /// stays because the drop shares the env's ending.
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
