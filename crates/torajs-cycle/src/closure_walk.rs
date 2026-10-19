//! Closure-env trace plumbing for the collector (RFC 20260717
//! closure-env-cycle knife 3) — the knife-2 trace ABI plus the two
//! trampolines `collect.rs`'s Closure arms dispatch through. Split
//! out of `collect.rs` (file-size hard limit). An async frame is
//! walked through the same ABI (`frame`); only where its trace is
//! kept differs.

use core::ffi::c_void;

use crate::children::PROPS_SLOT_INDEX;
use crate::frame::{TAG_ASYNC_FRAME, frame_trace_fn};
use crate::layout::{CLOSURE_PROPS_OFF, CLOSURE_TRACE_FN_OFF, HeapHeader, nan_box_is_cell_like};

/// The knife-2 trace ABI — `__env_trace_<fn>` (SSA-synthesized) and
/// `bound_trace` (method_bind) both have this shape; the visit
/// callback receives `(idx, child, slot_addr, ctx)` where `child` is
/// the RAW slot payload (possibly NaN-box bits) and `slot_addr` is
/// the address holding it (a capture-box value-slot for boxed
/// captures — writing 0 there breaks the edge without touching the
/// box's own lifetime).
pub(crate) type TraceFn = unsafe extern "C" fn(
    *mut c_void,
    unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    *mut c_void,
);

/// Read + transmute a traced cell's trace: a closure env's trace_fn
/// slot, or an async frame's desc row; None when 0 (leaf).
///
/// # Safety
/// `p` must be a live `TAG_CLOSURE` or `TAG_ASYNC_FRAME` cell.
pub(crate) unsafe fn cell_trace_fn(p: *mut c_void) -> Option<TraceFn> {
    if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_ASYNC_FRAME {
        return unsafe { frame_trace_fn(p) };
    }
    let raw = unsafe { *((p as *const u8).add(CLOSURE_TRACE_FN_OFF) as *const u64) };
    if raw == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<u64, TraceFn>(raw) })
    }
}

/// A traced cell's children: the slots its trace reports (none when
/// 0), then, for a closure env, the +24 expando props dict every
/// closure shares at a fixed offset. One body for every walk phase:
/// the slots reach the callback through the trampoline anyway.
///
/// # Safety
/// `p` must be a live `TAG_CLOSURE` or `TAG_ASYNC_FRAME` cell.
#[inline(never)]
pub(crate) unsafe fn for_each_traced_child(p: *mut c_void, f: &mut dyn FnMut(u64, *mut c_void)) {
    if let Some(trace) = unsafe { cell_trace_fn(p) } {
        let mut dyn_f: &mut dyn FnMut(u64, *mut c_void) = &mut *f;
        let ctx = &mut dyn_f as *mut &mut dyn FnMut(u64, *mut c_void) as *mut c_void;
        unsafe { trace(p, trace_visit_tramp, ctx) };
    }
    if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_ASYNC_FRAME {
        return;
    }
    let props = unsafe { *((p as *const u8).add(CLOSURE_PROPS_OFF) as *const *mut c_void) };
    if !props.is_null() {
        f(PROPS_SLOT_INDEX, props);
    }
}

/// Visit trampoline handing trace callbacks to the generic FnMut
/// walker. The NULL / NaN-box-immediate gate lives HERE — trace
/// bodies report raw slot payloads unfiltered (their contract,
/// mirroring `dynobj_child_at`'s in-arm filtering).
pub(crate) unsafe extern "C" fn trace_visit_tramp(
    idx: i64,
    child: *mut c_void,
    _slot_addr: *mut c_void,
    ctx: *mut c_void,
) {
    if child.is_null() || !nan_box_is_cell_like(child) {
        return;
    }
    let f = unsafe { &mut **(ctx as *mut &mut dyn FnMut(u64, *mut c_void)) };
    f(idx as u64, child);
}

/// Clear trampoline — zeroes the slot whose visit index matches the
/// target, implementing `clear_child_slot` for capture slots (the
/// index → address mapping only the trace body knows).
pub(crate) unsafe extern "C" fn trace_clear_tramp(
    idx: i64,
    _child: *mut c_void,
    slot_addr: *mut c_void,
    ctx: *mut c_void,
) {
    let target = unsafe { *(ctx as *const u64) };
    if idx as u64 == target {
        unsafe { *(slot_addr as *mut u64) = 0 };
    }
}
