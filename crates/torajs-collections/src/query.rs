//! Map / Set read-only queries — `size` / `has` / `get`.
//!
//! Port of `runtime_map.c::{__torajs_map_size, __torajs_map_has,
//! __torajs_map_get}` (P4.3-b, 2026-05-23). All three share
//! [`crate::probe::map_lookup_slot`] for hit detection; only `get`
//! also reads the entry's value fields.
//!
//! ## Borrowed-key ownership contract
//!
//! Per the C-side `map_drop_borrowed_key` helper: callers pass keys
//! ANY-tagged + with an rc_inc already applied (matches the
//! arr_push_any / dynobj_set contract). Query paths borrow the key
//! to do the lookup, then **release** that bump before returning —
//! they don't transfer ownership into the Map. Has / get must
//! `__torajs_value_drop_heap` the heap key on the way out; set does
//! NOT (the Map adopts the rc on fresh insert).
//!
//! Get additionally rc_inc's the returned heap value (caller becomes
//! the new owner of the returned reference).

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, ANY_UNDEF, Map};
use crate::probe::map_lookup_slot;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-anyvalue — NaN-box AnyValue decoders.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// Release caller's heap-key rc bump after a borrow-only lookup.
#[inline]
unsafe fn drop_borrowed_key(tag: i64, payload: i64) {
    if tag as u8 == ANY_HEAP {
        let p = payload as *mut c_void;
        if !p.is_null() {
            unsafe { __torajs_value_drop_heap(p) };
        }
    }
}

/// `__torajs_map_size(m)` — return `n_entries` (live count, excludes
/// tombstones). NULL `m` returns 0.
///
/// # Safety
/// `m` is null or a live Map heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_size(p: *const c_void) -> i64 {
    if p.is_null() {
        return 0;
    }
    let m = p as *const Map;
    unsafe { (*m).n_entries as i64 }
}

/// `__torajs_map_has(m, key_tag, key_payload)` — 1 if the key is
/// present, 0 otherwise. NULL `m` returns 0. Drops the caller's
/// borrowed heap-key rc before return.
///
/// # Safety
/// `m` is null or a live Map; for `ANY_HEAP` key, `key_payload` is
/// NULL or a valid heap pointer that the caller rc-bumped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_has(p: *const c_void, key_tag: i64, key_payload: i64) -> i64 {
    let r = if p.is_null() {
        0
    } else {
        let m = p as *const Map;
        let lr = unsafe { map_lookup_slot(m, key_tag as u8, key_payload as u64) };
        if lr.found { 1 } else { 0 }
    };
    unsafe { drop_borrowed_key(key_tag, key_payload) };
    r
}

/// `__torajs_map_get(m, key_tag, key_payload, *out_tag, *out_payload)`
/// — fills out-params with the bucket's stored value (rc-bumped if
/// heap) or with `(ANY_UNDEF, 0)` on miss / NULL `m`. Drops the
/// caller's borrowed heap-key rc before return.
///
/// # Safety
/// `m` is null or a live Map. `out_tag` / `out_payload` are valid
/// writable pointers (ssa_lower always passes valid out-params; null
/// allowed in tests for partial reads).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_get(
    p: *const c_void,
    key_tag: i64,
    key_payload: i64,
    out_tag: *mut i64,
    out_payload: *mut i64,
) {
    let (vt, vp) = if p.is_null() {
        (ANY_UNDEF as i64, 0i64)
    } else {
        let m = p as *const Map;
        let lr = unsafe { map_lookup_slot(m, key_tag as u8, key_payload as u64) };
        if !lr.found {
            (ANY_UNDEF as i64, 0i64)
        } else {
            let e = unsafe { (*m).entries.add(lr.entry_idx as usize) };
            let v_anyv = unsafe { (*e).value_anyv };
            let vt = unsafe { __torajs_anyv_unbox_tag(v_anyv) };
            let vp = unsafe { __torajs_anyv_unbox_value(v_anyv) };
            // Caller takes ownership of the returned heap ref.
            if vt as u8 == ANY_HEAP {
                let vp_ptr = vp as *mut c_void;
                if !vp_ptr.is_null() {
                    unsafe { __torajs_rc_inc(vp_ptr) };
                }
            }
            (vt, vp)
        }
    };
    if !out_tag.is_null() {
        unsafe { *out_tag = vt };
    }
    if !out_payload.is_null() {
        unsafe { *out_payload = vp };
    }
    unsafe { drop_borrowed_key(key_tag, key_payload) };
}

// ---- ES2025 Set read-only setops (§24.2.3.{12,13,14}) ----
//
// Set is stored as `Map<T, undefined>` in tora's runtime, so the
// readonly setops can walk the underlying MapEntry array directly
// and reuse the existing hash-map slot probe. None of them mutate
// either Set; the receiver / other refs are borrowed and not
// rc_dec'd here. Returns 1 / 0 (i64) to match the existing Set
// query convention (`__torajs_map_has`).

#[inline]
unsafe fn map_count(m: *const c_void) -> u32 {
    if m.is_null() {
        0
    } else {
        unsafe { (*(m as *const Map)).n_entries }
    }
}

/// `s.isSubsetOf(other)` — every member of `s` is also in `other`.
/// Empty `s` is a subset of anything (including null).
///
/// # Safety
/// `this` / `other` are null or live Set heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_is_subset_of(
    this: *const c_void,
    other: *const c_void,
) -> i64 {
    if unsafe { map_count(this) } == 0 {
        return 1;
    }
    if other.is_null() {
        return 0;
    }
    let m_this = unsafe { &*(this as *const Map) };
    let m_other = other as *const Map;
    for i in 0..m_this.n_used {
        let e = unsafe { &*m_this.entries.add(i as usize) };
        if e.hash == 0 {
            continue;
        }
        let k_anyv = e.key_anyv;
        let k_tag = unsafe { __torajs_anyv_unbox_tag(k_anyv) } as u8;
        let k_val = unsafe { __torajs_anyv_unbox_value(k_anyv) } as u64;
        let lr = unsafe { map_lookup_slot(m_other, k_tag, k_val) };
        if !lr.found {
            return 0;
        }
    }
    1
}

/// `s.isSupersetOf(other)` — every member of `other` is also in `s`.
/// Empty `other` is a subset of anything, so `s.isSupersetOf(empty)`
/// is true regardless of `s`.
///
/// # Safety
/// `this` / `other` are null or live Set heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_is_superset_of(
    this: *const c_void,
    other: *const c_void,
) -> i64 {
    if unsafe { map_count(other) } == 0 {
        return 1;
    }
    if this.is_null() {
        return 0;
    }
    let m_this = this as *const Map;
    let m_other = unsafe { &*(other as *const Map) };
    for i in 0..m_other.n_used {
        let e = unsafe { &*m_other.entries.add(i as usize) };
        if e.hash == 0 {
            continue;
        }
        let k_anyv = e.key_anyv;
        let k_tag = unsafe { __torajs_anyv_unbox_tag(k_anyv) } as u8;
        let k_val = unsafe { __torajs_anyv_unbox_value(k_anyv) } as u64;
        let lr = unsafe { map_lookup_slot(m_this, k_tag, k_val) };
        if !lr.found {
            return 0;
        }
    }
    1
}

/// `s.isDisjointFrom(other)` — no member of `s` appears in `other`.
/// Either Set being empty trivially satisfies the predicate.
///
/// # Safety
/// `this` / `other` are null or live Set heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_is_disjoint_from(
    this: *const c_void,
    other: *const c_void,
) -> i64 {
    if unsafe { map_count(this) } == 0 || unsafe { map_count(other) } == 0 {
        return 1;
    }
    let m_this = unsafe { &*(this as *const Map) };
    let m_other = other as *const Map;
    for i in 0..m_this.n_used {
        let e = unsafe { &*m_this.entries.add(i as usize) };
        if e.hash == 0 {
            continue;
        }
        let k_anyv = e.key_anyv;
        let k_tag = unsafe { __torajs_anyv_unbox_tag(k_anyv) } as u8;
        let k_val = unsafe { __torajs_anyv_unbox_value(k_anyv) } as u64;
        let lr = unsafe { map_lookup_slot(m_other, k_tag, k_val) };
        if lr.found {
            return 0;
        }
    }
    1
}
