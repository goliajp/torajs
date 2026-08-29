//! The two iterator cells' source, for the trial-deletion walk.
//!
//! `MapIter` and `ArrIter` each hold one strong reference to what
//! they iterate — `MapIter::map`, `ArrIter::arr` — taken so that
//! iteration stays valid past the caller's binding drop. Rotation 528
//! gave both tags their lazy property bag, and that bag has been the
//! only child the collector ever enumerated for them; the reference
//! the cell exists to hold was not an edge the walk could follow.
//!
//! An iterator that can be reached from its own source is an ordinary
//! ring — `const it = a.values(); a.push(it)` — two cells, no
//! closure, nothing exotic.
//!
//! Both layouts put the source at the same offset, so one arm serves
//! both tags:
//!
//! ```text
//! MapIter (40B)                  ArrIter (40B)
//! offset | field                 offset | field
//! -------|------                 -------|------
//!   0    | heap header             0    | heap header
//!   8    | map    — slot 0         8    | arr    — slot 0
//!  16    | cursor                 16    | cursor
//!  24    | kind + pad             24    | kind + family
//!  32    | props (the bag)        32    | props (the bag)
//! ```
//!
//! Mirrors `torajs-collections/src/iter.rs` and
//! `torajs-arr/src/iter.rs`. Unlike [`crate::proxy`]'s two slots
//! these are raw cell pointers, not NaN-boxed AnyValues, so the
//! read needs no immediate filter — a null pointer is the only
//! "nothing here" this slot can hold.
//!
//! Both tags are [`crate::layout_bag::corpse_takes_own_destructor`]
//! shapes, so a cleared slot is exactly what the corpse's own
//! destructor needs to see: `iter_drop` skips a null source rather
//! than releasing an edge this collect already accounted for.

use core::ffi::c_void;

use crate::layout_bag::{TAG_ARR_ITER, TAG_MAP_ITER};

/// Byte offset of `MapIter::map` / `ArrIter::arr`, directly after
/// the universal header in both layouts.
const ITER_SRC_OFF: usize = 8;

/// The child index [`crate::children::for_each_child`] yields the
/// source under. Neither iterator cell has an entry table, so index
/// 0 is free — the props bag keeps its own `PROPS_SLOT_INDEX`
/// sentinel.
pub const ITER_SRC_SLOT: u64 = 0;

/// True for the two stateful iterator cells that own their source.
#[inline]
pub fn is_iter_cell(type_tag: u16) -> bool {
    matches!(type_tag, TAG_MAP_ITER | TAG_ARR_ITER)
}

/// The Map or Array this iterator walks, or NULL once the source has
/// been released (a finished iterator, or a slot this collect has
/// already broken).
///
/// # Safety
/// `p` must be a live MapIter or ArrIter cell — check
/// [`is_iter_cell`] on its tag first.
#[inline]
pub unsafe fn iter_src(p: *mut c_void) -> *mut c_void {
    unsafe { *((p as *const u8).add(ITER_SRC_OFF) as *const *mut c_void) }
}

/// Null the source slot — `collect_white`'s first-sweep cycle break.
///
/// # Safety
/// Same as [`iter_src`].
#[inline]
pub unsafe fn iter_src_clear(p: *mut c_void) {
    unsafe { *((p as *mut u8).add(ITER_SRC_OFF) as *mut *mut c_void) = core::ptr::null_mut() };
}
