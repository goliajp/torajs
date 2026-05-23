//! Map / Set insert + overwrite — `__torajs_map_set`.
//!
//! Port of `runtime_map.c::__torajs_map_set` (P4.3-c, 2026-05-23).
//! Two grow paths: slot-side load (n_entries + n_tombstones + 1
//! crosses 3/4 of slots_count) doubles `slots[]`; entries[] exhaustion
//! (n_used >= entries_cap) doubles `entries[]`. Both go through
//! [`crate::probe::map_rehash`] which compacts dead entries.
//!
//! ## Ownership transitions
//!
//! - **Hit (overwrite)**: caller's key bump is released (key already
//!   owned by bucket); old value heap-rc is dropped; new value
//!   installed in-place.
//! - **Miss (fresh)**: caller's key bump transfers into the bucket
//!   (the bucket adopts ownership); new value installed as-is.

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, Map, MapEntry};
use crate::probe::{map_lookup_slot, map_rehash, map_slot_insert, slot_load_exceeded};

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `__torajs_map_set(m, key_tag, key_payload, value_tag, value_payload)`.
///
/// # Safety
/// `m` is null (early return) or a live Map. For `ANY_HEAP` key /
/// value, caller has rc-bumped the payload before the call (matches
/// arr_push_any contract).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_set(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
    value_tag: i64,
    value_payload: i64,
) {
    if p.is_null() {
        return;
    }
    let m = p as *mut Map;
    let kt = key_tag as u8;
    let vt = value_tag as u8;
    let kp = key_payload as u64;
    let vp = value_payload as u64;

    // Slot-side load: grow slot table if (entries + tombstones + 1)
    // would exceed 3/4. Grow entries[] in the same rehash if we're
    // also about to exhaust it (avoid back-to-back rehashes).
    unsafe {
        if slot_load_exceeded((*m).n_entries, (*m).n_tombstones, (*m).slots_count) {
            let new_slots = (*m).slots_count * 2;
            let mut new_entries = (*m).entries_cap;
            if (*m).n_entries + 1 > new_entries {
                new_entries *= 2;
            }
            map_rehash(m, new_entries, new_slots);
        }
    }

    let lr = unsafe { map_lookup_slot(m, kt, kp) };
    if lr.found {
        // Overwrite path. Drop old heap value + release caller's
        // borrowed heap key bump (bucket already owns the key).
        unsafe {
            let e = (*m).entries.add(lr.entry_idx as usize);
            if (*e).value_tag == ANY_HEAP {
                let old_vp = (*e).value_payload as *mut c_void;
                if !old_vp.is_null() {
                    __torajs_value_drop_heap(old_vp);
                }
            }
            if kt == ANY_HEAP {
                let new_kp = kp as *mut c_void;
                if !new_kp.is_null() {
                    __torajs_value_drop_heap(new_kp);
                }
            }
            (*e).value_tag = vt;
            (*e).value_payload = vp;
        }
        return;
    }

    // Fresh insert. Re-lookup after entries[]-grow rehash since indices
    // may have shifted.
    let (hash, new_idx) = unsafe {
        if (*m).n_used >= (*m).entries_cap {
            let new_entries_cap = (*m).entries_cap * 2;
            map_rehash(m, new_entries_cap, (*m).slots_count);
            let re = map_lookup_slot(m, kt, kp);
            (re.hash, (*m).n_used)
        } else {
            (lr.hash, (*m).n_used)
        }
    };

    unsafe {
        let e = (*m).entries.add(new_idx as usize) as *mut MapEntry;
        (*e).hash = hash;
        (*e).key_tag = kt;
        (*e).key_payload = kp;
        (*e).value_tag = vt;
        (*e).value_payload = vp;
        (*m).n_used += 1;
        (*m).n_entries += 1;
        // Always route the slot placement through robin-hood probing —
        // the `slot_idx` returned by lookup is an opportunistic
        // insert-candidate, but letting slot_insert do the proper
        // probe keeps the invariant that displaced cells get
        // robin-hood-swapped correctly. Tombstones compact at the
        // next rehash trigger.
        map_slot_insert((*m).slots, (*m).slots_count, hash, new_idx);
    }
}
