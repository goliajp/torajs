//! Map / Set entry removal — `delete` (per-key) + `clear` (whole-map).
//!
//! Port of `runtime_map.c::{__torajs_map_delete, __torajs_map_clear}`
//! (P4.3-d, 2026-05-24).
//!
//! ## delete
//!
//! Probe-then-tombstone:
//! 1. Lookup; miss → return 0.
//! 2. Hit → drop bucket's owned heap key + value refs.
//! 3. Mark entry-side tombstone (`hash = 0`) so iter walks skip; mark
//!    slot-side tombstone (`SLOT_TOMBSTONE`) so probe chains step past.
//! 4. `n_entries--` / `n_tombstones++`.
//! 5. Compact-rehash if slot-tombstone load exceeds `slots_count / 4`
//!    (lazy compaction; keeps probe chains short).
//! 6. Release caller's borrowed heap-key rc (matches has/get
//!    convention; mutate-side `set` does NOT release — bucket owns).
//!
//! ## clear
//!
//! Walk live entries → drop each pair's heap refs → memset entries[] →
//! reset slots[] to all-empty. Counts reset to zero. Reuses
//! allocations (no realloc — capacities unchanged).

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, ENTRY_HASH_TOMBSTONE, Map, MapEntry, SLOT_EMPTY, SLOT_TOMBSTONE, slot_make,
};
use crate::probe::{map_lookup_slot, map_rehash};

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Release a bucket's owning heap-tagged key + value refs (does
/// nothing for primitive-tagged entries). Used by both `delete`
/// (per-entry) and `clear` (per-entry walk).
#[inline]
unsafe fn drop_entry_refs(e: *mut MapEntry) {
    unsafe {
        if (*e).key_tag == ANY_HEAP {
            let kp = (*e).key_payload as *mut c_void;
            if !kp.is_null() {
                __torajs_value_drop_heap(kp);
            }
        }
        if (*e).value_tag == ANY_HEAP {
            let vp = (*e).value_payload as *mut c_void;
            if !vp.is_null() {
                __torajs_value_drop_heap(vp);
            }
        }
    }
}

/// Release caller's heap-key rc bump — borrow-only path (matches
/// `query::drop_borrowed_key`). Internal to delete.
#[inline]
unsafe fn drop_borrowed_key(tag: i64, payload: i64) {
    if tag as u8 == ANY_HEAP {
        let p = payload as *mut c_void;
        if !p.is_null() {
            unsafe { __torajs_value_drop_heap(p) };
        }
    }
}

/// `__torajs_map_delete(m, key_tag, key_payload)` — returns 1 iff a
/// bucket was actually removed.
///
/// # Safety
/// `m` is null or a live Map. For `ANY_HEAP` key, payload is NULL or
/// a valid heap pointer the caller rc-bumped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_delete(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
) -> i64 {
    let mut r: i64 = 0;
    if !p.is_null() {
        let m = p as *mut Map;
        let lr = unsafe { map_lookup_slot(m, key_tag as u8, key_payload as u64) };
        if lr.found {
            unsafe {
                let e = (*m).entries.add(lr.entry_idx as usize);
                drop_entry_refs(e);
                (*e).hash = ENTRY_HASH_TOMBSTONE;
                (*e).key_tag = 0;
                (*e).key_payload = 0;
                (*e).value_tag = 0;
                (*e).value_payload = 0;
                *(*m).slots.add(lr.slot_idx as usize) = slot_make(0, SLOT_TOMBSTONE);
                (*m).n_entries -= 1;
                (*m).n_tombstones += 1;
                if (*m).n_tombstones > (*m).slots_count / 4 {
                    // Compact entries[] + reset tombstones. Same cap,
                    // just rebuild — drops the slot-side tombstones
                    // accumulated since last compaction.
                    map_rehash(m, (*m).entries_cap, (*m).slots_count);
                }
            }
            r = 1;
        }
    }
    unsafe { drop_borrowed_key(key_tag, key_payload) };
    r
}

/// `__torajs_map_clear(m)` — drop every live entry; reset slots /
/// counts. Reuses the existing allocations.
///
/// # Safety
/// `m` is null or a live Map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_clear(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let m = p as *mut Map;
    unsafe {
        let n_used = (*m).n_used;
        for i in 0..n_used as usize {
            let e = (*m).entries.add(i);
            if (*e).hash == ENTRY_HASH_TOMBSTONE {
                continue;
            }
            drop_entry_refs(e);
        }
        // Zero the entries[] prefix that was used (capacity-many is
        // safe too — calloc-initialized tail is already zero).
        core::ptr::write_bytes((*m).entries, 0, (*m).entries_cap as usize);
        // Reset every slot to `(0, SLOT_EMPTY)`.
        for k in 0..(*m).slots_count as usize {
            *(*m).slots.add(k) = slot_make(0, SLOT_EMPTY);
        }
        (*m).n_entries = 0;
        (*m).n_used = 0;
        (*m).n_tombstones = 0;
    }
}
