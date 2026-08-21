//! The "is any weak observer alive" gate on the rc-hit-zero path.
//!
//! When a refcount transitions to zero, any live `WeakRef` /
//! `WeakMap` / `WeakSet` observing that cell has to learn of it
//! before the memory goes away — torajs-weak's
//! `__torajs_weakref_target_dying` walks its registry and clears the
//! observers. That walk is gated on a live-observer count, so a
//! program without weak observers pays only an untaken branch inside
//! it. The branch was inside the callee, though, and the callee is in
//! another static archive: every hit-zero paid a full opaque call to
//! go and learn that there was nothing to do (3.5% of `split-only-100k`
//! for seven inline substrings released per iteration, rotation 469;
//! the same tax on every dying cell in every program).
//!
//! The count now lives here, on the rc side, as one exported
//! zero-initialised word: torajs-weak adjusts it through the symbol
//! when an observer is registered or dropped, and [`notify_target_dying`]
//! reads it before calling out. Same counter, same semantics, read on
//! the side that already has the branch's operands in registers.
//!
//! Exported under a C name rather than a Rust path so that the copy
//! of this crate baked into every runtime archive resolves to ONE
//! word at link time — the in-house linker resolves exported symbols
//! by name across archives; a crate-private static would be
//! internalised per archive and each archive would count on its own.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

/// Live weak-observer count for the whole process. Written only by
/// torajs-weak (register / unregister / target-dying walk).
#[unsafe(no_mangle)]
pub static __torajs_weakref_active: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    /// torajs-weak — walk the observers of `target` and clear them.
    fn __torajs_weakref_target_dying(target: *mut c_void);
}

/// Fire the target-dying walk if — and only if — some weak observer
/// is alive anywhere in the program.
///
/// # Safety
///
/// `target` is the header of a cell whose refcount just reached zero
/// and whose memory is still valid.
#[inline]
pub(crate) unsafe fn notify_target_dying(target: *mut c_void) {
    if __torajs_weakref_active.load(Ordering::Relaxed) != 0 {
        unsafe { __torajs_weakref_target_dying(target) };
    }
}
