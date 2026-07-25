//! DynObj key deletion.
//!
//! Spec §10.1.10 OrdinaryDelete. Drops the entry's owning key cell
//! + any ANY_HEAP value, turns the dense entry into a hole
//! (`key_ptr_tagged = 0`, skipped by iteration / drop / compact) and
//! the index slot into [`IDX_TOMBSTONE`] (probe walks past), then
//! decrements `count`. The hole is reclaimed when the dense array
//! fills and [`crate::resize`] compacts; re-inserting the key later
//! appends at the tail — JS "delete then set moves to the end" order.

use core::ffi::c_void;

use crate::layout::{DYNOBJ_KEY_HOLE, IDX_TOMBSTONE};
use crate::probe::{bucket_key_ptr, count, drop_key, entries, index_ptr, probe, set_count};

unsafe extern "C" {
    /// Cross-tier — universal heap-value drop (NaN-box-safe; the 7d-A
    /// cell gate filters immediate AnyValues so this is unconditional).
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// `__torajs_dynobj_delete(obj, key)` — remove `key` from `obj`.
/// Returns 1 iff an entry was actually deleted, 0 otherwise (NULL
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
    unsafe {
        let e = entries(obj).add(pr.entry as usize);
        // Drop the owning key cell + any heap-pointer value. The NaN-
        // box cell gate inside __torajs_value_drop_heap filters out
        // immediate AnyValues so we can call it unconditionally.
        drop_key(bucket_key_ptr((*e).key_ptr_tagged));
        __torajs_value_drop_heap((*e).value_anyv as *mut c_void);
        (*e).key_ptr_tagged = DYNOBJ_KEY_HOLE;
        (*e).value_anyv = 0;
        *index_ptr(obj).add(pr.slot as usize) = IDX_TOMBSTONE;
        set_count(obj, count(obj) - 1);
    }
    1
}
