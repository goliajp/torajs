//! DynObj universal-drop entry.
//!
//! Called by `__torajs_value_drop_heap`'s `Tag::DynObj` dispatch case
//! (torajs-value-drop) when a dynobj's refcount transitions to zero.
//!
//! Walks the dense entry array in order, drops each live entry's key
//! cell + any ANY_HEAP value (holes are skipped), then frees the
//! store block and the header cell.

use core::ffi::c_void;

use crate::layout::{DYNOBJ_HEADER_BYTES, DYNOBJ_KEY_HOLE, store_bytes};
use crate::probe::{bucket_key_ptr, cap, drop_key, entries, entries_len, store_ptr};

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount dec. Returns 1 iff the
    /// caller should free + walk children; 0 otherwise.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// Cross-tier — universal heap-value drop (NaN-box-safe; the 7d-A
    /// cell gate filters immediate AnyValues so this is unconditional).
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
    /// torajs-cycle — register a possible cycle root when the rc
    /// stayed positive (RFC 20260717 blade 2: dynobj joined the
    /// trial-deletion walk; mirror of torajs-arr drop.rs).
    fn __torajs_cycle_buffer(p: *mut c_void);
    /// torajs-cycle — scrub a normal-dropped block from the root
    /// buffer before its memory is freed / pool-recycled (a stale
    /// entry dangles into the next collect).
    fn __torajs_cycle_unbuffer(p: *mut c_void);
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
        // Still referenced — a dict that lost one of several refs is
        // a possible cycle root (RFC 20260717 blade 2; the buffer's
        // FLAG_BUFFERED gate dedups repeats).
        unsafe { __torajs_cycle_buffer(obj) };
        return;
    }
    // Normal-drop to rc=0 — scrub any stale root-buffer entry before
    // the block is freed or pool-recycled below.
    unsafe { __torajs_cycle_unbuffer(obj) };
    let len = unsafe { entries_len(obj) };
    let ent = unsafe { entries(obj) };
    for i in 0..len as usize {
        let kp_tagged = unsafe { (*ent.add(i)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        unsafe {
            drop_key(bucket_key_ptr(kp_tagged));
            __torajs_value_drop_heap((*ent.add(i)).value_anyv as *mut c_void);
        }
    }
    let obj_cap = unsafe { cap(obj) };
    // Round 5 attack #3 — never-grown objects recycle through the
    // LIFO pool as a header+store pair (the store stays wired on the
    // header's store pointer; the allocator re-inits header / counts
    // / index on pop). Grown objects carry a different-size store and
    // take the plain free path.
    if obj_cap == crate::layout::DYNOBJ_INITIAL_CAP {
        if let Some(nn) = core::ptr::NonNull::new(obj as *mut u8) {
            if crate::pool::push(nn) {
                return;
            }
        }
    }
    unsafe {
        free(store_ptr(obj) as *mut c_void, store_bytes(obj_cap));
        free(obj, DYNOBJ_HEADER_BYTES);
    }
}
