//! `new WeakRef(target)` — allocate a fresh +1-rc WeakRef and
//! register the (target, WeakRef) tuple with the shared observer
//! registry so a future strong-rc-zero on `target` clears this
//! WeakRef's target slot to NULL.
//!
//! Port of `runtime_weakref.c::__torajs_weakref_create` (P4.3'-a,
//! 2026-05-24). The target is intentionally NOT rc_inc'd — that's
//! the whole point of "weak" reference; the registry observes it
//! out-of-band instead.

use core::ffi::c_void;

use crate::layout::{HeapHeader, OBSERVER_WEAKREF, TAG_WEAKREF, WeakRef};

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;

    /// Defined in `runtime_weakref.c`. Adds an ObserverNode keyed by
    /// `target` to the process-global `g_buckets[1024]` table.
    /// Tolerant of NULL target (early-returns).
    fn __torajs_weakref_registry_register(target: *mut c_void, kind: u32, owner: *mut c_void);
}

/// `__torajs_weakref_create(target)` — allocate a fresh empty
/// `+1`-rc WeakRef observing `target`.
///
/// # Safety
/// `target` may be NULL (returns a WeakRef whose `target` field is
/// already NULL and which is NOT registered) or any live heap
/// pointer. Returned pointer is owned by the caller; release via
/// `__torajs_weakref_drop` which the universal
/// `__torajs_value_drop_heap` dispatch routes to under TAG_WEAKREF.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_create(target: *mut c_void) -> *mut c_void {
    let wr = unsafe { malloc(core::mem::size_of::<WeakRef>()) } as *mut WeakRef;
    unsafe {
        (*wr).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_WEAKREF,
            flags: 0,
        };
        (*wr).target = target;
        if !target.is_null() {
            __torajs_weakref_registry_register(target, OBSERVER_WEAKREF, wr as *mut c_void);
        }
    }
    wr as *mut c_void
}
