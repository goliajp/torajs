//! DynObj universal-drop entry.
//!
//! Called by `__torajs_value_drop_heap`'s `Tag::DynObj` dispatch case
//! (torajs-value-drop) when a dynobj's refcount transitions to zero.
//!
//! Walks the dense entry array in order, drops each live entry's key
//! Str + any ANY_HEAP value (holes are skipped), then frees the block.

use core::ffi::c_void;

use crate::layout::{DYNOBJ_KEY_HOLE, block_bytes};
use crate::probe::{bucket_key_ptr, cap, entries, entries_len};

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
    let len = unsafe { entries_len(obj) };
    let ent = unsafe { entries(obj) };
    for i in 0..len as usize {
        let kp_tagged = unsafe { (*ent.add(i)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        unsafe {
            __torajs_str_drop(bucket_key_ptr(kp_tagged));
            __torajs_value_drop_heap((*ent.add(i)).value_anyv as *mut c_void);
        }
    }
    let obj_cap = unsafe { cap(obj) };
    // Round 5 attack #3 — never-grown blocks recycle through the
    // LIFO pool (uniform 168-byte size class); the allocator re-inits
    // header/counts/index on pop. Grown blocks have a different byte
    // size and take the plain free path.
    if obj_cap == crate::layout::DYNOBJ_INITIAL_CAP {
        if let Some(nn) = core::ptr::NonNull::new(obj as *mut u8) {
            if crate::pool::push(nn) {
                return;
            }
        }
    }
    unsafe {
        free(obj, block_bytes(obj_cap));
    }
}
