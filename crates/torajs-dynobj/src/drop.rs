//! DynObj universal-drop entry.
//!
//! Port of `runtime_str.c::__torajs_dynobj_drop` (P4.2-e, 2026-05-23).
//! Called by the C-side `__torajs_value_drop_heap` dispatch case
//! `__TORAJS_TAG_DYNOBJ` (still in runtime_str.c) when a dynobj's
//! refcount transitions to zero.
//!
//! Walks every live bucket, drops the key Str + any ANY_HEAP value,
//! then libc-frees the block. Tombstones and empties are skipped.

use core::ffi::c_void;

use crate::layout::{DYNOBJ_BUCKET_SIZE, DYNOBJ_HDR_SIZE, DYNOBJ_KEY_EMPTY, DYNOBJ_KEY_TOMBSTONE};
use crate::probe::{bucket_key_ptr, buckets, cap};

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount dec. Returns 1 iff the
    /// caller should free + walk children; 0 otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    fn __torajs_str_drop(s: *mut c_void);
    /// Cross-tier — universal heap-value drop (NaN-box-safe; the 7d-A
    /// cell gate filters immediate AnyValues so this is unconditional).
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
}

/// `__torajs_dynobj_drop(obj)` — universal heap-value drop.
///
/// # Safety
/// `obj` is null or a live dynobj heap pointer. After return, the
/// pointee may be freed (last-owner path) — caller must drop the
/// reference.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_drop(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(obj) } == 0 {
        return;
    }
    let cap = unsafe { cap(obj) };
    let bk = unsafe { buckets(obj) };
    for i in 0..cap as usize {
        let kp_tagged = unsafe { (*bk.add(i)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_EMPTY || kp_tagged == DYNOBJ_KEY_TOMBSTONE {
            continue;
        }
        unsafe {
            __torajs_str_drop(bucket_key_ptr(kp_tagged));
            __torajs_value_drop_heap((*bk.add(i)).value_anyv as *mut c_void);
        }
    }
    // Layer 1 sized free: obj block = DYNOBJ_HDR_SIZE + cap *
    // DYNOBJ_BUCKET_SIZE; cap is in scope from line 42.
    let total = DYNOBJ_HDR_SIZE + (cap as usize) * DYNOBJ_BUCKET_SIZE;
    unsafe {
        free(obj, total);
    }
}
