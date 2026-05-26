//! DynObj implicit-set — `obj.x = v` + object-literal init.
//!
//! Port of `runtime_str.c::__torajs_dynobj_set` (P4.2-c, 2026-05-23).
//! Implements spec §10.1.5.1 OrdinarySet → §10.1.6.2 CreateDataProperty
//! for the "writable=true" path; throws TypeError when overwriting a
//! non-writable bucket (`__torajs_throw_type_error` records pending +
//! returns; caller's ssa-lower-side `emit_throw_check` propagates).
//!
//! Fresh inserts: rc-bump the key (bucket owns its share) + write
//! default flags (writable / enumerable / configurable all true).
//! Existing bucket overwrite: drop the old heap value if ANY_HEAP,
//! preserve the existing flag bits, swap only the low-8 ANY_TAG + value.

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, BUCKET_FLAG_WRITABLE, BUCKET_FLAGS_DEFAULT, BUCKET_TAG_MASK, DYNOBJ_KEY_TOMBSTONE,
};
use crate::probe::{bucket_flags, bucket_make_key_tagged, buckets, probe};
use crate::resize::resize;

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount inc. Bucket takes ownership
    /// of the key string on fresh insert.
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — torajs-throw's TypeError thrower. Records pending
    /// throw via TLS + returns normally; caller MUST explicitly
    /// `return;` after invoking (per `feedback_throw_extern_returns_void`).
    fn __torajs_throw_type_error(msg: *const u8);

    /// Cross-tier — heap-value drop dispatch (NaN-box-safe; immediate
    /// AnyValues are filtered by the 7d-A cell gate). Drops the old
    /// bucket value when overwriting.
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
    let cap = unsafe { *((obj as *const u8).add(12) as *const u32) };
    let count = unsafe { *((obj as *const u8).add(8) as *const u32) };
    let tomb = unsafe { *((obj as *const u8).add(16) as *const u32) };

    // Load-factor guard: keep `(count + tomb + 1) <= cap * 7/8` after
    // this insert. Mirrors C's `* 8 > cap * 7` integer-arithmetic form.
    if (count + tomb + 1) * 8 > cap * 7 {
        unsafe {
            resize(obj_slot, cap * 2);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let bk = unsafe { buckets(obj) };
    if pr.found {
        let cur_kp_tagged = unsafe { (*bk.add(pr.idx as usize)).key_ptr_tagged };
        let cur_flags = bucket_flags(cur_kp_tagged);
        if cur_flags & BUCKET_FLAG_WRITABLE == 0 {
            unsafe {
                __torajs_throw_type_error(
                    c"TypeError: Cannot assign to read only property".as_ptr() as *const u8,
                );
            }
            return;
        }
        let cur_value_anyv = unsafe { (*bk.add(pr.idx as usize)).value_anyv };
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
            (*bk.add(pr.idx as usize)).value_anyv =
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
        }
    } else {
        // Fresh insert path. Reusing a tombstone slot decrements `tomb`.
        if unsafe { (*bk.add(pr.idx as usize)).key_ptr_tagged } == DYNOBJ_KEY_TOMBSTONE {
            unsafe {
                *((obj as *mut u8).add(16) as *mut u32) = tomb - 1;
            }
        }
        unsafe {
            __torajs_rc_inc(key);
            (*bk.add(pr.idx as usize)).key_ptr_tagged =
                bucket_make_key_tagged(key, BUCKET_FLAGS_DEFAULT);
            (*bk.add(pr.idx as usize)).value_anyv =
                __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
            *((obj as *mut u8).add(8) as *mut u32) = count + 1;
        }
    }
}
