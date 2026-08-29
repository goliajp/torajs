//! `Tag::Proxy` mirror for the trial-deletion walk.
//!
//! A Proxy owns two AnyValue slots, `[[ProxyTarget]]` and
//! `[[ProxyHandler]]`, and both are ordinary objects. A handler that
//! closes over the proxy it serves — a logging trap that records
//! which proxy it fired for, a revocable pair that keeps its own
//! proxy — is a ring made of exactly two cells, and it was invisible:
//! the collector had no arm for the tag at all, so a Proxy was
//! neither a root nor a place the walk could pass through.
//!
//! Mirror of `torajs-anyvalue/src/proxy.rs`:
//!
//! ```text
//! Proxy cell (24B)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | target  (AnyValue) — slot 0
//!  16    | handler (AnyValue) — slot 1
//! ```
//!
//! Revocation writes the null sentinel into both slots, which is
//! what §10.5.4.1 says to do, so a revoked proxy simply walks as
//! zero children — no separate flag to keep in step.

use core::ffi::c_void;

use crate::layout::{FLAG_STATIC_LITERAL, HeapHeader, nan_box_is_cell_like};

/// `torajs_rc::Tag::Proxy` mirror — the stable wire value.
pub const TAG_PROXY: u16 = 26;

/// `[[ProxyTarget]]`; `[[ProxyHandler]]` sits 8 bytes on.
const PROXY_TARGET_OFF: usize = 8;

/// Walkable slot count — target and handler, always both.
pub const PROXY_CHILD_COUNT: u64 = 2;

/// True when `p` is a Proxy cell with at least one live slot. A
/// revoked proxy holds the null sentinel in both and has nothing to
/// walk, the same way a bagless Date does.
///
/// # Safety
/// `p` is NULL or a live heap block with the universal header.
#[inline]
pub unsafe fn is_visitable_proxy(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 || header.type_tag != TAG_PROXY {
        return false;
    }
    (0..PROXY_CHILD_COUNT).any(|i| !unsafe { proxy_child_at(p, i) }.is_null())
}

/// The cell slot `i` holds (0 = target, 1 = handler), or NULL when
/// that slot is the null sentinel or any other immediate.
///
/// # Safety
/// `p` must be a live Proxy cell and `i` below [`PROXY_CHILD_COUNT`].
#[inline]
pub unsafe fn proxy_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let v = unsafe { *(slot_ptr(p, i) as *const u64) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero slot `i` — `collect_white`'s first-sweep cycle break. A zero
/// word is not a cell to this walk and not a cell to
/// `__torajs_anyv_rc_dec` either, so the corpse's own destructor in
/// [`crate::defer`] pass B skips the slot rather than releasing an
/// edge this collect already accounted for.
///
/// # Safety
/// Same as [`proxy_child_at`].
#[inline]
pub unsafe fn proxy_slot_clear(p: *mut c_void, i: u64) {
    unsafe { *slot_ptr(p, i) = 0 };
}

#[inline]
unsafe fn slot_ptr(p: *mut c_void, i: u64) -> *mut u64 {
    unsafe { (p as *mut u8).add(PROXY_TARGET_OFF + i as usize * 8) as *mut u64 }
}
