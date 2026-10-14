//! DynObj entry attribute + hole surface — `set_entry_flags` /
//! `set_entry_hole` / `entry_is_hole`: the kernels that change WHAT AN
//! ENTRY IS (its descriptor bits, or whether it is a shadow-domain
//! tombstone) rather than what it holds. Split from `set.rs` (file-size
//! hard limit, RFC 20260912-global-var-cell); bodies verbatim.

use core::ffi::c_void;

use crate::probe::{
    Entry, bucket_make_key_tagged, count, entries, entries_cap, entries_len, index_ptr, probe,
    set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Raw attribute-flags upsert — RFC 20260712-arr-exotic-define
/// chunk B. The Array DefineOwnProperty kernel (torajs-arr) stores
/// per-index attribute flags as shadow entries in the array's expando
/// dynobj; it has already run the §10.1.6.3 validation, so this
/// bypasses `dynobj_define`'s checks. A hit rewrites only the flag
/// bits in `key_ptr_tagged`; a miss inserts a fresh entry whose value
/// slot is dead (`undefined` — the element storage owns the value).
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj (non-NULL — the caller allocates). `key` is a live Str
/// heap pointer. `flags` uses the `BUCKET_FLAG_*` bit positions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_flags(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    flags: u64,
) {
    let obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe { resize(obj) };
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let key_ptr =
            (unsafe { (*e).key_ptr_tagged } & crate::layout::BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe { (*e).key_ptr_tagged = bucket_make_key_tagged(key_ptr, flags) };
        // A flags upsert revives a hole entry (RFC 20260713 chunk C):
        // the dead value slot resets to the live-shadow spelling.
        unsafe { (*e).value_anyv = __torajs_anyv_box_from_pair(5, 0) };
    } else {
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, flags),
                value_anyv: __torajs_anyv_box_from_pair(5, 0),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
}

/// Raw value swap on an EXISTING entry, attributes untouched — RFC
/// 20260912-global-var-cell knife 4: `__torajs_global_var_bind`
/// declaring a `var` over a pre-existing READ-ONLY data property (the
/// §16.1.7 "already an own property: do nothing" branch, which for
/// one storage means the entry's value becomes the cell over the slot
/// that now holds the property's value). `set` would refuse the
/// non-writable entry and `define` would run §10.1.6.3 against the
/// new value; both are right for a program's write and wrong for this
/// re-plumbing, which changes what the entry IS, not what the program
/// sees. The old payload is dropped; `value` is the caller's
/// transferred reference. A miss does nothing.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_set_entry_flags`]; `(tag,
/// value)` is a live entry payload the caller hands over.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_value_raw(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    let obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    if !pr.found {
        return;
    }
    let e = unsafe { entries(obj).add(pr.entry as usize) };
    unsafe {
        __torajs_value_drop_heap((*e).value_anyv as *mut c_void);
        (*e).value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
    }
}

/// Shadow-entry HOLE sentinel (RFC 20260713-defprop-tpd-cluster
/// chunk C) — `delete arr[i]` marks the index absent while element
/// storage stays dense. A hole entry is a shadow entry whose dead
/// value slot holds [`crate::layout::DYNOBJ_HOLE_SENTINEL`] instead
/// of the live `VALUE_UNDEFINED`; flag bits read as 0 (w/e/c all
/// false is a distinct, legal live state — the sentinel
/// disambiguates).
///
/// # Safety
/// Same contract as [`__torajs_dynobj_set_entry_flags`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_entry_hole(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
) {
    let obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe { resize(obj) };
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let key_ptr =
            (unsafe { (*e).key_ptr_tagged } & crate::layout::BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe { (*e).key_ptr_tagged = bucket_make_key_tagged(key_ptr, 0) };
        // Drop the entry's current value before the sentinel lands —
        // deleting an accessor index would otherwise leak the pair
        // (RFC 20260713 chunk C; NaN-box-safe, immediates no-op).
        unsafe { __torajs_value_drop_heap((*e).value_anyv as *mut c_void) };
        unsafe { (*e).value_anyv = crate::layout::DYNOBJ_HOLE_SENTINEL };
    } else {
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, 0),
                value_anyv: crate::layout::DYNOBJ_HOLE_SENTINEL,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
}

/// Whether `key`'s entry is a HOLE sentinel (see
/// [`__torajs_dynobj_set_entry_hole`]). Absent key answers 0.
///
/// # Safety
/// `obj` is null or a live dynobj heap pointer; `key` a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_entry_is_hole(
    obj: *const c_void,
    key: *const c_void,
) -> i32 {
    if obj.is_null() {
        return 0;
    }
    let pr = unsafe { probe(obj, key) };
    if !pr.found {
        return 0;
    }
    let v = unsafe { (*entries(obj).add(pr.entry as usize)).value_anyv };
    (v == crate::layout::DYNOBJ_HOLE_SENTINEL) as i32
}
