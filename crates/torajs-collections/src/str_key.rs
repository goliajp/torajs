//! String-keyed Map access with a borrowed key — the kernels a
//! `Map` call site lowers to when its key is statically a string.
//!
//! The generic `has` / `get` / `set` take the key as an owned
//! `(tag, payload)` pair: the call site spends two extern calls
//! (`__torajs_anyv_str_slot_tag` / `_value`) and an `rc_inc` building
//! it, and a query kernel spends a `value_drop_heap` releasing it.
//! None of that is needed to look a string up (map-str-100k
//! decomposition 2026-09-17, S4 + S5: 9 ns of a 134 ns iteration).
//!
//! The lookup is also split from the entry access. `find` answers an
//! entry index; `entry_get` consumes one, and `set_str` takes one as a
//! checked hint.
//! A compare-and-branch on the index is what `has` is, and an index
//! found earlier on the same Map and key, with nothing writing the Map
//! in between, is still the key's entry — which is what lets a later
//! pass share one lookup among `has`, `get` and `set` on the same key
//! (JSC's DFG does the same through `MapHash` / `MapGet` CSE).
//!
//! Key shapes: a string slot holds NULL (JS `null`), the torajs-str
//! `undefined` sentinel cell, or a heap Str; they map to the same
//! pairs `__torajs_anyv_str_slot_tag` answers.

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, ANY_NULL, ANY_UNDEF, ENTRY_HASH_TOMBSTONE, Map};
use crate::probe::map_lookup_slot;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_map_set(
        p: *mut c_void,
        key_tag: i64,
        key_payload: i64,
        value_tag: i64,
        value_payload: i64,
    );
    static __TORAJS_STR_UNDEF_CELL: u8;
}

/// The `(tag, payload)` a string slot stands for.
#[inline]
fn str_key_pair(s: *const u8) -> (u8, u64) {
    if s.is_null() {
        (ANY_NULL, 0)
    } else if core::ptr::eq(s, unsafe { &__TORAJS_STR_UNDEF_CELL }) {
        (ANY_UNDEF, 0)
    } else {
        (ANY_HEAP, s as u64)
    }
}

/// `__torajs_map_find_str(m, s)` — the entry index holding key `s`, or
/// -1 (also for a NULL `m`).
///
/// # Safety
/// `m` is null or a live Map; `s` is a borrowed string slot value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_find_str(p: *const c_void, s: *const u8) -> i64 {
    if p.is_null() {
        return -1;
    }
    let (kt, kp) = str_key_pair(s);
    let lr = unsafe { map_lookup_slot(p as *const Map, kt, kp) };
    if lr.found { lr.entry_idx as i64 } else { -1 }
}

/// `__torajs_map_entry_get(m, idx, out_tag, out_payload)` — the value
/// pair of entry `idx` with the caller owning a heap value's new ref,
/// or `(ANY_UNDEF, 0)` for `idx < 0`.
///
/// # Safety
/// `m` is a live Map when `idx >= 0`, and `idx` is a live entry index
/// answered by [`__torajs_map_find_str`] with no write to `m` since.
/// The out pointers are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_entry_get(
    p: *const c_void,
    idx: i64,
    out_tag: *mut i64,
    out_payload: *mut i64,
) {
    let (vt, vp) = if idx < 0 {
        (ANY_UNDEF, 0)
    } else {
        let m = p as *const Map;
        let e = unsafe { (*m).entries.add(idx as usize) };
        let pair = crate::key_decode::stored_pair(unsafe { (*e).value_anyv });
        if pair.0 == ANY_HEAP && pair.1 != 0 {
            unsafe { __torajs_rc_inc(pair.1 as *mut c_void) };
        }
        pair
    };
    unsafe {
        *out_tag = vt as i64;
        *out_payload = vp as i64;
    }
}

/// Entry `hint` is live and keyed by the string `s`.
///
/// # Safety
/// `m` is a live Map.
unsafe fn hint_holds(m: *const Map, hint: i64, s: *const u8) -> bool {
    if hint >= unsafe { (*m).n_used } as i64 {
        return false;
    }
    let e = unsafe { &*(*m).entries.add(hint as usize) };
    if e.hash == ENTRY_HASH_TOMBSTONE {
        return false;
    }
    let (kt, kp) = str_key_pair(s);
    let (et, ep) = crate::key_decode::stored_pair(e.key_anyv);
    unsafe { crate::eq::map_keys_equal(et, ep, kt, kp) }
}

/// `__torajs_map_set_str(m, s, hint, value_tag, value_payload)` —
/// `m.set(s, v)` with a borrowed key. The value pair is owned (its
/// ref moves into the Map). `hint` is an entry index a find on `s`
/// answered earlier, or -1. It is checked, not trusted: code between
/// the find and this call may have written the Map, so the hint is
/// used only while it is still a live entry whose key equals `s`;
/// anything else takes the full lookup.
///
/// # Safety
/// `m` is null or a live Map; `s` is a borrowed string slot value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_set_str(
    p: *mut c_void,
    s: *const u8,
    hint: i64,
    value_tag: i64,
    value_payload: i64,
) {
    if p.is_null() {
        return;
    }
    if hint >= 0 && unsafe { hint_holds(p as *const Map, hint, s) } {
        let m = p as *mut Map;
        unsafe {
            let e = (*m).entries.add(hint as usize);
            let (old_t, old_p) = crate::key_decode::stored_pair((*e).value_anyv);
            (*e).value_anyv = __torajs_anyv_box_from_pair(value_tag, value_payload);
            if old_t == ANY_HEAP && old_p != 0 {
                __torajs_value_drop_heap(old_p as *mut c_void);
            }
        }
        return;
    }
    let (kt, kp) = str_key_pair(s);
    // the generic kernel takes an owned key: it adopts the ref on
    // insert and releases it on overwrite
    if kt == ANY_HEAP {
        unsafe { __torajs_rc_inc(kp as *mut c_void) };
    }
    unsafe { __torajs_map_set(p, kt as i64, kp as i64, value_tag, value_payload) };
}
