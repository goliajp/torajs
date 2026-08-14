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

use crate::layout::{ANY_F64, ANY_HEAP, Map, MapEntry};
use crate::probe::{map_lookup_slot, map_rehash, map_slot_insert, slot_load_exceeded};

unsafe extern "C" {
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-anyvalue — NaN-box AnyValue pair encoder / tag decoder.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
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
    let mut kp = key_payload as u64;
    let vp = value_payload as u64;

    // Spec §24.1.3.9 / §24.2.4.1: if key is -0, set key to +0 before
    // storing (iteration must observe +0, not -0).
    if kt == ANY_F64 && kp == (-0.0f64).to_bits() {
        kp = 0;
    }

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
            let cur_value_tag = __torajs_anyv_unbox_tag((*e).value_anyv) as u8;
            if cur_value_tag == ANY_HEAP {
                let old_vp = __torajs_anyv_unbox_value((*e).value_anyv) as *mut c_void;
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
            (*e).value_anyv = __torajs_anyv_box_from_pair(vt as i64, vp as i64);
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
        (*e).key_anyv = __torajs_anyv_box_from_pair(kt as i64, kp as i64);
        (*e).value_anyv = __torajs_anyv_box_from_pair(vt as i64, vp as i64);
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

/// `map.getOrInsert(key, default)` per the stage-3 upsert proposal
/// (bun ships it — RFC 20260721-builtin-method-reflection 刀 6):
/// a present key answers its CURRENT value untouched; a missing key
/// inserts `default` and answers it. The hit path is a single
/// robin-hood lookup; the miss path delegates to
/// [`__torajs_map_set`] so both grow/rehash legs stay single-source.
///
/// ## Ownership
///
/// Caller hands ONE owned stake per heap `key` / `default` (the
/// `pair_consumed` contract). Hit: both stakes are released here
/// (bucket already owns its key; `default` is unused). Miss: both
/// transfer into the bucket through `map_set`. The value written to
/// `out_*` is rc-bumped for the caller either way.
///
/// # Safety
/// `m` is null (answers undefined) or a live Map; `out_*` are
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_get_or_insert(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
    default_tag: i64,
    default_payload: i64,
    out_tag: *mut i64,
    out_payload: *mut i64,
) {
    let release = |tag: i64, payload: i64| {
        if tag as u8 == ANY_HEAP && payload != 0 {
            unsafe { __torajs_value_drop_heap(payload as *mut c_void) };
        }
    };
    if p.is_null() {
        release(key_tag, key_payload);
        release(default_tag, default_payload);
        unsafe {
            *out_tag = 5;
            *out_payload = 0;
        }
        return;
    }
    let m = p as *mut Map;
    let mut kp = key_payload as u64;
    // §24.1.3.9 -0 → +0 key normalization (matches map_set).
    if key_tag as u8 == ANY_F64 && kp == (-0.0f64).to_bits() {
        kp = 0;
    }
    let lr = unsafe { map_lookup_slot(m, key_tag as u8, kp) };
    if lr.found {
        unsafe {
            let e = (*m).entries.add(lr.entry_idx as usize);
            let v_anyv = (*e).value_anyv;
            let vt = __torajs_anyv_unbox_tag(v_anyv);
            let vp = __torajs_anyv_unbox_value(v_anyv);
            if vt as u8 == ANY_HEAP && vp != 0 {
                __torajs_rc_inc(vp as *mut c_void);
            }
            *out_tag = vt;
            *out_payload = vp;
        }
        release(key_tag, kp as i64);
        release(default_tag, default_payload);
        return;
    }
    // Miss — insert the default (both stakes transfer), then answer
    // it (+1 for the caller).
    unsafe {
        __torajs_map_set(p, key_tag, kp as i64, default_tag, default_payload);
        if default_tag as u8 == ANY_HEAP && default_payload != 0 {
            __torajs_rc_inc(default_payload as *mut c_void);
        }
        *out_tag = default_tag;
        *out_payload = default_payload;
    }
}

/// Non-consuming lookup for the upsert-computed composition (383-04):
/// the key is BORROWED (no stake changes hands — the caller keeps its
/// own for a possible insert after the callback), and a hit's value
/// is rc-bumped for the caller. `out_found` distinguishes a present
/// `undefined` from a miss, which `get`'s sentinel cannot.
///
/// # Safety
/// `p` is null (answers not-found) or a live Map; `out_*` are
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_peek(
    p: *mut c_void,
    key_tag: i64,
    key_payload: i64,
    out_found: *mut i64,
    out_tag: *mut i64,
    out_payload: *mut i64,
) {
    unsafe {
        *out_found = 0;
        *out_tag = 5;
        *out_payload = 0;
        if p.is_null() {
            return;
        }
        let m = p as *mut Map;
        let mut kp = key_payload as u64;
        // §24.1.3.9 -0 → +0 key normalization (matches map_set).
        if key_tag as u8 == ANY_F64 && kp == (-0.0f64).to_bits() {
            kp = 0;
        }
        let lr = map_lookup_slot(m, key_tag as u8, kp);
        if !lr.found {
            return;
        }
        let e = (*m).entries.add(lr.entry_idx as usize);
        let v_anyv = (*e).value_anyv;
        let vt = __torajs_anyv_unbox_tag(v_anyv);
        let vp = __torajs_anyv_unbox_value(v_anyv);
        if vt as u8 == ANY_HEAP && vp != 0 {
            __torajs_rc_inc(vp as *mut c_void);
        }
        *out_found = 1;
        *out_tag = vt;
        *out_payload = vp;
    }
}
