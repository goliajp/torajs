//! DynObj implicit-set — `obj.x = v` + object-literal init.
//!
//! Implements spec §10.1.5.1 OrdinarySet → §10.1.6.2 CreateDataProperty
//! for the "writable=true" path; throws TypeError when overwriting a
//! non-writable entry (`__torajs_throw_type_error` records pending +
//! returns; caller's ssa-lower-side `emit_throw_check` propagates).
//!
//! Fresh inserts append to the dense entry array (insertion order) and
//! point the probed index slot (first tombstone, else first empty) at
//! the new entry: rc-bump the key (entry owns its share) + write
//! default flags (writable / enumerable / configurable all true).
//! Existing entry overwrite: drop the old heap value if ANY_HEAP,
//! preserve the existing flag bits, swap only the NaN-box value.

use core::ffi::c_void;

use crate::accessor::{__torajs_accessor_invoke_setter, value_is_accessor};
use crate::layout::{ANY_HEAP, BUCKET_FLAG_WRITABLE, BUCKET_FLAGS_DEFAULT, BUCKET_TAG_MASK};
use crate::probe::{
    Entry, bucket_flags, bucket_make_key_tagged, count, entries, entries_cap, entries_len,
    index_ptr, probe, set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount inc. Entry takes ownership
    /// of the key string on fresh insert.
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — torajs-throw's TypeError thrower. Records pending
    /// throw via TLS + returns normally; caller MUST explicitly
    /// `return;` after invoking (per `feedback_throw_extern_returns_void`).
    fn __torajs_throw_type_error(msg: *const u8);

    /// Cross-tier — heap-value drop dispatch (NaN-box-safe; immediate
    /// AnyValues are filtered by the 7d-A cell gate). Drops the old
    /// entry value when overwriting.
    fn __torajs_value_drop_heap(child: *mut c_void);

    /// torajs-anyvalue — NaN-box AnyValue pair encoder. Takes (tag,
    /// value) as i64 + returns the packed u64 immediate.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;

    /// torajs-anyvalue — NaN-box AnyValue tag decoder.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
}

/// `__torajs_dynobj_set(obj_slot, key, tag, value)` — implicit-set entry.
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj or NULL. `key` is a live Str heap pointer. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    // Dense-array-full guard: compact (and grow if genuinely full)
    // before probing so a fresh insert always has an append slot.
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let cur_value_anyv = unsafe { (*e).value_anyv };
        // RFC C3 — accessor entry: dispatch the setter, never a data
        // write (checked before the writable gate — accessors carry no
        // writable bit, so the data path would wrongly throw "read
        // only"). `cur_value_anyv` is the AccessorPair cell verbatim.
        if unsafe { value_is_accessor(cur_value_anyv) } {
            let value_anyv = unsafe {
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64)
            };
            if unsafe {
                __torajs_accessor_invoke_setter(cur_value_anyv as *const c_void, value_anyv)
            } == 0
            {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr() as *const u8,
                    );
                }
            }
            return;
        }
        let cur_flags = bucket_flags(unsafe { (*e).key_ptr_tagged });
        if cur_flags & BUCKET_FLAG_WRITABLE == 0 {
            unsafe {
                __torajs_throw_type_error(
                    c"Attempted to assign to readonly property.".as_ptr() as *const u8
                );
            }
            return;
        }
        let cur_tag = unsafe { __torajs_anyv_unbox_tag(cur_value_anyv) } as u64;
        // Drop the old heap value if the current slot was ANY_HEAP.
        // (The NaN-box cell gate in __torajs_value_drop_heap would
        // also short-circuit immediates, but checking tag first keeps
        // the hot path one extern call lighter on the common case.)
        if cur_tag == ANY_HEAP {
            unsafe {
                __torajs_value_drop_heap(cur_value_anyv as *mut c_void);
            }
        }
        // Preserve existing flag bits in key_ptr_tagged; rebox the
        // (tag, value) pair into a fresh NaN-box AnyValue.
        unsafe {
            (*e).value_anyv =
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
        }
    } else {
        // Fresh insert: append to the dense array (insertion order),
        // point the probed slot (tombstone reuse or empty) at it.
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, BUCKET_FLAGS_DEFAULT),
                value_anyv: __torajs_anyv_box_from_pair(
                    (tag & BUCKET_TAG_MASK) as i64,
                    value as i64,
                ),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
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
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }
    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };
    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let key_ptr =
            (unsafe { (*e).key_ptr_tagged } & crate::layout::BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe { (*e).key_ptr_tagged = bucket_make_key_tagged(key_ptr, flags) };
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
