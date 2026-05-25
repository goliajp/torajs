//! `Map` / `Set` allocation.
//!
//! Port of `runtime_map.c::__torajs_map_create` (P4.3-a, 2026-05-23).
//! Allocates a fresh `Map` struct + `slots[16]` array (all empty) +
//! `entries[8]` array (zero-init). Returns a `+1`-rc heap pointer.
//!
//! Set and Map share this entry point — Set is purely an SSA-side
//! distinction (same layout, same TAG_MAP).

use core::ffi::c_void;

use crate::layout::{
    HeapHeader, MAP_ENTRIES_INITIAL, MAP_SLOTS_INITIAL, Map, MapEntry, MapSlot, SLOT_EMPTY,
    TAG_MAP, slot_make,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat malloc/calloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_calloc"]
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
}

/// `__torajs_map_create()` — allocate a fresh empty Map/Set.
///
/// Initial shape: `slots[16]` all `SLOT_EMPTY`, `entries[8]` zeroed,
/// `n_entries = n_used = n_tombstones = 0`, header `refcount=1`,
/// `type_tag = TAG_MAP`.
///
/// # Safety
/// Returned pointer is owned by the caller; release via
/// `__torajs_map_drop` (lands in P4.3-g) which the universal
/// `__torajs_value_drop_heap` dispatch routes to under TAG_MAP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_create() -> *mut c_void {
    let m = unsafe { malloc(core::mem::size_of::<Map>()) } as *mut Map;
    unsafe {
        (*m).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_MAP,
            flags: 0,
        };
        (*m).n_entries = 0;
        (*m).n_used = 0;
        (*m).entries_cap = MAP_ENTRIES_INITIAL;
        (*m).slots_count = MAP_SLOTS_INITIAL;
        (*m).n_tombstones = 0;
        (*m)._pad = 0;

        let slots =
            malloc(MAP_SLOTS_INITIAL as usize * core::mem::size_of::<MapSlot>()) as *mut MapSlot;
        for k in 0..MAP_SLOTS_INITIAL as usize {
            *slots.add(k) = slot_make(0, SLOT_EMPTY);
        }
        (*m).slots = slots;

        // entries[] zero-init — calloc gives `hash = 0 = ENTRY_HASH_TOMBSTONE`
        // for every slot, which is also the "no live entry yet" state until
        // `n_used` grows. Code paths never read past `n_used` so this is fine.
        (*m).entries = calloc(
            MAP_ENTRIES_INITIAL as usize,
            core::mem::size_of::<MapEntry>(),
        ) as *mut MapEntry;
    }
    m as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{slot_hash, slot_index};

    #[test]
    fn create_inits_header_and_arrays() {
        let p = unsafe { __torajs_map_create() } as *mut Map;
        assert!(!p.is_null());
        unsafe {
            assert_eq!((*p).header.refcount, 1);
            assert_eq!((*p).header.type_tag, TAG_MAP);
            assert_eq!((*p).header.flags, 0);
            assert_eq!((*p).n_entries, 0);
            assert_eq!((*p).n_used, 0);
            assert_eq!((*p).entries_cap, MAP_ENTRIES_INITIAL);
            assert_eq!((*p).slots_count, MAP_SLOTS_INITIAL);
            assert_eq!((*p).n_tombstones, 0);
            assert!(!(*p).slots.is_null());
            assert!(!(*p).entries.is_null());

            // Every slot is SLOT_EMPTY (low 32) + hash=0 (high 32).
            for i in 0..MAP_SLOTS_INITIAL as usize {
                let s = *(*p).slots.add(i);
                assert_eq!(slot_index(s), SLOT_EMPTY);
                assert_eq!(slot_hash(s), 0);
            }

            // Hand the arrays + struct back to mmalloc (test-only path —
            // production drop is map_drop in drop.rs).
            unsafe extern "C" {
                #[link_name = "__torajs_libc_free"]
                fn free(p: *mut c_void);
            }
            free((*p).slots as *mut c_void);
            free((*p).entries as *mut c_void);
            free(p as *mut c_void);
        }
    }
}
