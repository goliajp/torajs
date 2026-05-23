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

use crate::layout::{ANY_HEAP, BUCKET_TAG_MASK, DYNOBJ_TOMBSTONE};
use crate::probe::{buckets, cap};

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount dec. Returns 1 iff the
    /// caller should free + walk children; 0 otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn free(p: *mut c_void);
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
        let kp = unsafe { (*bk.add(i)).key_ptr };
        if kp.is_null() || kp == DYNOBJ_TOMBSTONE {
            continue;
        }
        unsafe {
            __torajs_str_drop(kp);
        }
        let tag = unsafe { (*bk.add(i)).tag };
        if tag & BUCKET_TAG_MASK == ANY_HEAP {
            let val = unsafe { (*bk.add(i)).value } as *mut c_void;
            unsafe {
                __torajs_value_drop_heap(val);
            }
        }
    }
    unsafe {
        free(obj);
    }
}
