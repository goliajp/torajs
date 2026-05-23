//! `wr.deref()` — return the observed target, or NULL if it has
//! been reclaimed. On the alive path, the caller takes ownership
//! of a new strong reference, so we `rc_inc` first.
//!
//! Port of `runtime_weakref.c::__torajs_weakref_deref` (P4.3'-a,
//! 2026-05-24).

use core::ffi::c_void;

use crate::layout::WeakRef;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
}

/// `__torajs_weakref_deref(wr)` — return the observed target with
/// +1 strong rc on the alive path, NULL after reclamation.
///
/// # Safety
/// `wr` is NULL or a live WeakRef heap pointer. The pointer
/// itself is not consumed (no rc dec on `wr`). The returned
/// pointer, if non-NULL, is a freshly +1-rc'd strong reference
/// the caller owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_deref(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return core::ptr::null_mut();
    }
    let wr = p as *mut WeakRef;
    let t = unsafe { (*wr).target };
    if !t.is_null() {
        unsafe { __torajs_rc_inc(t) };
    }
    t
}
