//! Map / Set entry-table mirror for the trial-deletion walk — the
//! [`crate::dynobj`] twin for the insertion-ordered hash table.
//!
//! Rotation 528 gave these two shapes a walk through their
//! own-property bag and left the entry table itself out, recording
//! it as a documented under-collection: `m.set("k", m)` was a ring
//! the collector could not see. This is the layout mirror that
//! closes it.
//!
//! Mirror of `torajs-collections/src/layout.rs` — only the fields the
//! walk reads are named here, the same narrow replication
//! [`crate::dynobj`] uses:
//!
//! ```text
//! Map / Set cell (56B; Set is layout-identical to Map)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!  12    | n_used (u32) — entries[] occupied prefix, tombstones included
//!  40    | entries (*mut MapEntry)
//!  48    | props (*mut DynObj) — the lazy own-property bag
//!
//! MapEntry (24B)
//! offset | field
//! -------|------
//!   0    | hash (u32) — 0 is the entry-side tombstone
//!   8    | key_anyv (u64)   — NaN-box
//!  16    | value_anyv (u64) — NaN-box
//! ```
//!
//! Slot index `i` addresses entry `i / 2`, its key when `i` is even
//! and its value when odd. A key and a value are two separate edges
//! even when they point at the same cell — the Map owns a reference
//! for each, so the walk must trial-decrement each.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;

/// `n_used: u32` — the occupied prefix of `entries[]`, tombstones
/// included (iteration order is insertion order, so there is no
/// compaction to skip).
const MAP_N_USED_OFF: usize = 12;
/// `entries: *mut MapEntry`.
const MAP_ENTRIES_OFF: usize = 40;
/// Per-entry stride: `hash: u32` + pad + `key_anyv: u64` +
/// `value_anyv: u64`.
const MAP_ENTRY_SIZE: usize = 24;
/// `key_anyv` offset within an entry; `value_anyv` sits 8 bytes on.
const MAP_ENTRY_KEY_OFF: usize = 8;
/// A zero hash marks an entry `delete` already released — its key and
/// value slots are stale bits, not owned references.
const ENTRY_HASH_TOMBSTONE: u32 = 0;

/// Walkable slot count: two per occupied entry. Tombstoned entries
/// keep their index (so the count is an upper bound and
/// [`map_child_at`] answers NULL for them), exactly the way
/// `dynobj_entries_len` counts holes.
///
/// # Safety
/// `p` must be a live Map or Set heap pointer.
#[inline]
pub unsafe fn map_child_count(p: *mut c_void) -> u64 {
    unsafe { *((p as *const u8).add(MAP_N_USED_OFF) as *const u32) as u64 * 2 }
}

/// The `key_anyv` / `value_anyv` word slot `i` addresses, or NULL
/// when the entry is a tombstone or the table was never allocated.
#[inline]
unsafe fn slot_ptr(p: *mut c_void, i: u64) -> *mut u64 {
    let base = unsafe { *((p as *const u8).add(MAP_ENTRIES_OFF) as *const *mut u8) };
    if base.is_null() {
        return core::ptr::null_mut();
    }
    let e = unsafe { base.add((i / 2) as usize * MAP_ENTRY_SIZE) };
    if unsafe { *(e as *const u32) } == ENTRY_HASH_TOMBSTONE {
        return core::ptr::null_mut();
    }
    unsafe { e.add(MAP_ENTRY_KEY_OFF + (i % 2) as usize * 8) as *mut u64 }
}

/// The cell slot `i` holds, or NULL for a tombstone or a NaN-box
/// immediate — the per-slot gate [`crate::dynobj::dynobj_child_at`]
/// applies, for the same reason: an entry value carrying `42` is not
/// a pointer.
///
/// # Safety
/// `p` must be a live Map or Set heap pointer and `i` an index
/// [`map_child_count`] admits.
pub unsafe fn map_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let s = unsafe { slot_ptr(p, i) };
    if s.is_null() {
        return core::ptr::null_mut();
    }
    let v = unsafe { *s } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero slot `i` — `collect_white`'s first-sweep cycle break. A zero
/// word reads as no-child through the cell-like gate here and as
/// `Null` through `__torajs_anyv_unbox_tag`, so the corpse's own
/// destructor in [`crate::defer`] pass B skips the slot instead of
/// releasing an edge the walk already accounted.
///
/// # Safety
/// Same as [`map_child_at`].
pub unsafe fn map_slot_clear(p: *mut c_void, i: u64) {
    let s = unsafe { slot_ptr(p, i) };
    if !s.is_null() {
        unsafe { *s = 0 };
    }
}
