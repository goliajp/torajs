//! WeakRef universal-drop entry — `__torajs_weakref_drop`.
//!
//! Port of `runtime_weakref.c::__torajs_weakref_drop` (P4.3'-a,
//! 2026-05-24). Routed via `runtime_str.c::value_drop_heap`'s
//! `TAG_WEAKREF` case when a WeakRef's refcount transitions to zero.
//!
//! Unlike strong-ref drops, the work here is two-fold:
//!   1. Decrement the WeakRef's own refcount; if not last owner, return.
//!   2. On last owner: deregister the (target, WEAKREF, owner=wr) tuple
//!      from the shared observer registry so the next dying-target walk
//!      doesn't dispatch through a stale pointer. Then libc-free the
//!      WeakRef struct itself.
//!
//! `STATIC_LITERAL` flag short-circuits the whole path — mirror of
//! the C body's `flags & 4` early-return. (WeakRef literals aren't
//! emitted today, but the bit-check is invariant across all heap
//! types via the universal heap-header flag scheme.)

use core::ffi::c_void;

use crate::layout::{FLAG_STATIC_LITERAL, OBSERVER_WEAKREF, WeakRef};

unsafe extern "C" {
    fn free(p: *mut c_void);

    /// Defined in `runtime_weakref.c`. Removes the matching
    /// (target, kind, owner) tuple from the shared observer
    /// registry. Tolerant of NULL target and missing entries.
    fn __torajs_weakref_registry_deregister(target: *mut c_void, kind: u32, owner: *mut c_void);
}

/// `__torajs_weakref_drop(wr)` — refcount-aware WeakRef drop.
/// Returns immediately on NULL or STATIC_LITERAL; otherwise
/// decrements refcount and, on transition to zero, deregisters
/// from the observer registry and frees the WeakRef struct.
///
/// # Safety
/// `p` is NULL or a live WeakRef heap pointer. After return, the
/// pointee may be freed (last-owner path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let wr = p as *mut WeakRef;
    unsafe {
        if (*wr).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        (*wr).header.refcount -= 1;
        if (*wr).header.refcount == 0 {
            let target = (*wr).target;
            if !target.is_null() {
                __torajs_weakref_registry_deregister(target, OBSERVER_WEAKREF, p);
            }
            free(p);
        }
    }
}
