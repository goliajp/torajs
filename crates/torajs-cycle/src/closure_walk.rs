//! Closure-env trace plumbing for the collector (RFC 20260717
//! closure-env-cycle knife 3) — the knife-2 trace ABI plus the two
//! trampolines `collect.rs`'s Closure arms dispatch through. Split
//! out of `collect.rs` (file-size hard limit).

use core::ffi::c_void;

use crate::layout::{CLOSURE_TRACE_FN_OFF, nan_box_is_cell_like};

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

/// Read + transmute the env's trace_fn slot; None when 0 (leaf).
///
/// # Safety
/// `p` must be a live `TAG_CLOSURE` cell.
pub(crate) unsafe fn closure_trace_fn(p: *mut c_void) -> Option<TraceFn> {
    let raw = unsafe { *((p as *const u8).add(CLOSURE_TRACE_FN_OFF) as *const u64) };
    if raw == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<u64, TraceFn>(raw) })
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
