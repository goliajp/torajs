//! DynObj key deletion.
//!
//! Port of `runtime_str.c::__torajs_dynobj_delete` (P4.2-e, 2026-05-23).
//! Spec §10.1.10 OrdinaryDelete. Drops the bucket's owning key Str
//! + any ANY_HEAP value, replaces key_ptr with the tombstone sentinel
//! so the probe walk continues past it, and rebalances count / tomb.

use core::ffi::c_void;

use crate::layout::DYNOBJ_KEY_TOMBSTONE;
use crate::probe::{bucket_key_ptr, buckets, probe};

unsafe extern "C" {
    /// Cross-tier — torajs-str's Str drop (releases the bucket's
    /// owning key share).
    fn __torajs_str_drop(s: *mut c_void);

    /// Cross-tier — universal heap-value drop (NaN-box-safe; the 7d-A
    /// cell gate filters immediate AnyValues so this is unconditional).
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// `__torajs_dynobj_delete(obj, key)` — remove `key` from `obj`.
/// Returns 1 iff a bucket was actually deleted, 0 otherwise (NULL
/// `obj` or key absent).
///
/// # Safety
/// `obj` is null or a live dynobj heap pointer. `key` (if reached)
/// is a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32 {
    if obj.is_null() {
        return 0;
    }
    let pr = unsafe { probe(obj, key) };
    if !pr.found {
        return 0;
    }
    let bk = unsafe { buckets(obj) };
    unsafe {
        let bucket = &mut *bk.add(pr.idx as usize);
        // Drop the owning key Str + any heap-pointer value. The NaN-
        // box cell gate inside __torajs_value_drop_heap filters out
        // immediate AnyValues so we can call it unconditionally.
        __torajs_str_drop(bucket_key_ptr(bucket.key_ptr_tagged));
        __torajs_value_drop_heap(bucket.value_anyv as *mut c_void);
        bucket.key_ptr_tagged = DYNOBJ_KEY_TOMBSTONE;
        bucket.value_anyv = 0;
    }
    // count-- / tomb++.
    unsafe {
        let count_p = (obj as *mut u8).add(8) as *mut u32;
        let tomb_p = (obj as *mut u8).add(16) as *mut u32;
        *count_p -= 1;
        *tomb_p += 1;
    }
    1
}
