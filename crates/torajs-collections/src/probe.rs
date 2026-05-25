//! Map slot table — robin-hood probing + rehash.
//!
//! Port of `runtime_map.c::{map_slot_insert, map_lookup_slot, map_rehash}`
//! (P4.3-b, 2026-05-23). Pure-Rust internals; the C-side `static`
//! copies stay until their consumer fns port (P4.3-c..-g).
//!
//! ## Robin-hood rules
//!
//! - **Probe distance** for a resident slot `s` at index `i` is
//!   `(i + cap - (hash(s) & mask)) & mask`.
//! - **Insert**: if the incoming key's probe distance exceeds the
//!   resident's, swap and continue with the displaced cell. Keeps
//!   max probe distance bounded by `log2(N)` typical.
//! - **Lookup early-termination**: if we out-probe the resident at
//!   `i` and it's NOT a tombstone, the key isn't anywhere later in
//!   the chain (since insertion would have displaced this resident).
//!   Robin-hood's signature optimization vs linear probing.
//!
//! Tombstones (slot-side `SLOT_TOMBSTONE`) are walked past on lookup
//! and remembered as the first reinsert candidate; rehash drops them.

use core::ffi::c_void;

use crate::layout::{
    ENTRY_HASH_TOMBSTONE, MAP_LOAD_DENOM, MAP_LOAD_NUMER, Map, MapEntry, MapSlot, SLOT_EMPTY,
    SLOT_TOMBSTONE, slot_hash, slot_index, slot_make,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat alloc/free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_calloc"]
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// Insert `(hash, entry_idx)` into a freshly-prepared `slots[cap]`
/// array. Caller must size the array so load stays below threshold
/// (rehash ensures this for the live mutation paths).
///
/// # Safety
/// `slots` valid for `cap` writable `MapSlot` cells; `cap` is power
/// of 2; `hash` is non-zero (per `map_hash_key` contract); `entry_idx`
/// is a live index into the caller's `entries[]`.
pub(crate) unsafe fn map_slot_insert(slots: *mut MapSlot, cap: u32, hash: u32, idx: u32) {
    let mask = cap - 1;
    let mut i = hash & mask;
    let mut probe: u32 = 0;
    let mut cur_hash = hash;
    let mut cur_idx = idx;
    loop {
        let s = unsafe { *slots.add(i as usize) };
        if slot_index(s) == SLOT_EMPTY {
            unsafe { *slots.add(i as usize) = slot_make(cur_hash, cur_idx) };
            return;
        }
        // Robin-hood: if the resident is closer to its ideal slot than
        // we are, displace it.
        let slot_ideal = slot_hash(s) & mask;
        let slot_probe = (i + cap - slot_ideal) & mask;
        if slot_probe < probe {
            let old_hash = slot_hash(s);
            let old_idx = slot_index(s);
            unsafe { *slots.add(i as usize) = slot_make(cur_hash, cur_idx) };
            cur_hash = old_hash;
            cur_idx = old_idx;
            probe = slot_probe;
        }
        i = (i + 1) & mask;
        probe += 1;
    }
}

/// Lookup outcome from [`map_lookup_slot`]. `slot_idx` is the
/// preferred reinsert slot on miss (first tombstone if seen, else
/// first empty); on hit it's the live key's slot. Currently only
/// `delete.rs` (P4.3-d) will read `slot_idx` directly to tombstone
/// it — query / set use `found` + `entry_idx` + `hash`.
#[allow(dead_code)]
pub(crate) struct LookupResult {
    pub slot_idx: u32,
    pub entry_idx: u32,
    pub hash: u32,
    pub found: bool,
}

/// Walk slot table looking for `(tag, payload)`. Returns hash +
/// either the live slot/entry index (on hit) or the insertion target
/// (on miss).
///
/// # Safety
/// `m` is a live `Map`; for `ANY_HEAP` payload, the heap pointer is
/// NULL or has a universal header.
pub(crate) unsafe fn map_lookup_slot(m: *const Map, tag: u8, payload: u64) -> LookupResult {
    let hash = unsafe { crate::hash::map_hash_key(tag, payload) };
    let slots_count = unsafe { (*m).slots_count };
    let mask = slots_count - 1;
    let mut i = hash & mask;
    let mut probe: u32 = 0;
    let mut first_tomb: u32 = slots_count; // sentinel "not seen yet"
    loop {
        let s = unsafe { *(*m).slots.add(i as usize) };
        let s_idx = slot_index(s);
        if s_idx == SLOT_EMPTY {
            return LookupResult {
                slot_idx: if first_tomb != slots_count {
                    first_tomb
                } else {
                    i
                },
                entry_idx: SLOT_EMPTY,
                hash,
                found: false,
            };
        }
        if s_idx == SLOT_TOMBSTONE {
            if first_tomb == slots_count {
                first_tomb = i;
            }
        } else if slot_hash(s) == hash {
            let e = unsafe { (*m).entries.add(s_idx as usize) };
            let eq =
                unsafe { crate::eq::map_keys_equal((*e).key_tag, (*e).key_payload, tag, payload) };
            if eq {
                return LookupResult {
                    slot_idx: i,
                    entry_idx: s_idx,
                    hash,
                    found: true,
                };
            }
        } else {
            // Robin-hood early termination: if we've out-probed this
            // resident and it isn't a tombstone, the key isn't later
            // in the chain either.
            let slot_ideal = slot_hash(s) & mask;
            let slot_probe = (i + slots_count - slot_ideal) & mask;
            if slot_probe < probe {
                return LookupResult {
                    slot_idx: if first_tomb != slots_count {
                        first_tomb
                    } else {
                        i
                    },
                    entry_idx: SLOT_EMPTY,
                    hash,
                    found: false,
                };
            }
        }
        i = (i + 1) & mask;
        probe += 1;
        if probe >= slots_count {
            return LookupResult {
                slot_idx: if first_tomb != slots_count {
                    first_tomb
                } else {
                    i
                },
                entry_idx: SLOT_EMPTY,
                hash,
                found: false,
            };
        }
    }
}

/// Rebuild `m`'s slot table + compact `entries[]` (drop entry-side
/// tombstones, preserve insertion order in the new array). Called
/// when `n_used` exhausts `entries_cap` or load factor crosses
/// threshold.
///
/// # Safety
/// `m` is a live `Map`; `new_entries_cap` ≥ live entry count;
/// `new_slots_count` is a power of 2 ≥ `MAP_SLOTS_INITIAL`.
pub(crate) unsafe fn map_rehash(m: *mut Map, new_entries_cap: u32, new_slots_count: u32) {
    let old_e = unsafe { (*m).entries };
    let old_used = unsafe { (*m).n_used };
    let new_e = unsafe {
        calloc(new_entries_cap as usize, core::mem::size_of::<MapEntry>()) as *mut MapEntry
    };
    let new_s = unsafe {
        malloc(new_slots_count as usize * core::mem::size_of::<MapSlot>()) as *mut MapSlot
    };
    for k in 0..new_slots_count as usize {
        unsafe { *new_s.add(k) = slot_make(0, SLOT_EMPTY) };
    }
    let mut new_used: u32 = 0;
    for k in 0..old_used as usize {
        let src = unsafe { old_e.add(k) };
        let src_hash = unsafe { (*src).hash };
        if src_hash == ENTRY_HASH_TOMBSTONE {
            continue;
        }
        // Copy entry as-is (Bitwise-copy is OK; refcounts unchanged,
        // we're just relocating the holding cell).
        unsafe { *new_e.add(new_used as usize) = core::ptr::read(src) };
        unsafe { map_slot_insert(new_s, new_slots_count, src_hash, new_used) };
        new_used += 1;
    }
    unsafe {
        free(old_e as *mut c_void);
        free((*m).slots as *mut c_void);
        (*m).entries = new_e;
        (*m).slots = new_s;
        (*m).entries_cap = new_entries_cap;
        (*m).slots_count = new_slots_count;
        (*m).n_used = new_used;
        (*m).n_tombstones = 0;
        // n_entries unchanged — live count is invariant under rehash.
    }
}

/// Slot-side load check: does inserting one more entry push us over
/// the `MAP_LOAD_NUMER / MAP_LOAD_DENOM` (3/4) threshold?
///
/// Uses `n_entries + n_tombstones` (live entries + slot-side
/// tombstones) — both occupy a slot, so both count against load.
/// Distinct from `n_used` (which counts ENTRY-side tombstones not
/// slot-side ones; entries[] grow path uses n_used separately).
#[inline]
pub(crate) fn slot_load_exceeded(n_entries: u32, n_tombstones: u32, slots_count: u32) -> bool {
    (n_entries + n_tombstones + 1) * MAP_LOAD_DENOM > slots_count * MAP_LOAD_NUMER
}
