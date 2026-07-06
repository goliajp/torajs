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
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
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

/// Chunk 629 — boxed variant for the statically-typed surface: the
/// checker types `wr.deref()` as `Nullable<Any>`, so the SSA value
/// is an AnyValue box. An alive target's box IS its pointer (NaN-box
/// heap contract, +1 strong rc the caller owns); a cleared/reclaimed
/// target answers the undefined box per ES §26.1.4.2.
///
/// # Safety
/// Same contract as [`__torajs_weakref_deref`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_deref_any(p: *mut c_void) -> u64 {
    let t = unsafe { __torajs_weakref_deref(p) };
    if t.is_null() {
        unsafe { __torajs_anyv_box_from_pair(5, 0) }
    } else {
        t as u64
    }
}
