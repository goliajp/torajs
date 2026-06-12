//! DynObj table resize — fresh allocation + compact + index rebuild.
//!
//! Private-to-crate helper used by [`crate::set`] / [`crate::define`]
//! when the dense entry array fills (`entries_len == entries_cap`).
//!
//! Algorithm (CPython `dictresize` shape):
//! 1. Pick the smallest power-of-2 cap with `cap > count * 3` (at
//!    least [`DYNOBJ_INITIAL_CAP`]). When most entries are holes this
//!    lands on the same or a smaller cap — the pass degenerates to a
//!    pure compact; when the table is genuinely full it doubles+.
//! 2. calloc a fresh block, copy the heap header verbatim (preserves
//!    refcount + type_tag + flags), fill the new index with IDX_EMPTY.
//! 3. Walk the old dense array in order; skip holes; append each live
//!    entry to the new dense array (insertion order preserved) and
//!    probe the new index for its slot (all distinct keys ⇒ lands on
//!    an empty slot). Tombstones and holes vanish.
//! 4. Update `*obj_slot` + free the old block.

use core::ffi::c_void;

use crate::layout::{
    DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF, DYNOBJ_ENTRIES_CAP_OFF, DYNOBJ_ENTRIES_LEN_OFF,
    DYNOBJ_INDEX_OFF, DYNOBJ_INITIAL_CAP, DYNOBJ_KEY_HOLE, block_bytes, entries_cap_for,
};
use crate::probe::{Entry, bucket_key_ptr, cap, count, entries, entries_len, index_ptr, probe};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat calloc/free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
}

/// Rebuild `*obj_slot` into a fresh block sized for its live count,
/// compacting holes out of the dense array and rebuilding the hash
/// index. Old block is freed. Guarantees at least one free dense slot
/// on return.
///
/// # Safety
/// `obj_slot` must point at a non-NULL `*mut c_void` that itself holds
/// a live dynobj heap pointer.
pub(crate) unsafe fn resize(obj_slot: *mut *mut c_void) {
    let old = unsafe { *obj_slot };
    let old_cap = unsafe { cap(old) };
    let old_len = unsafe { entries_len(old) };
    let old_ent = unsafe { entries(old) };
    let live = unsafe { count(old) };

    // CPython growth rule: smallest power-of-2 cap > count * 3. The
    // 7/8 dense ratio then gives entries_cap > count, so the pending
    // insert always fits (debug-asserted below).
    let mut new_cap = DYNOBJ_INITIAL_CAP;
    while (new_cap as u64) <= (live as u64) * 3 {
        new_cap *= 2;
    }
    debug_assert!(entries_cap_for(new_cap) > live);

    let p = unsafe { calloc(block_bytes(new_cap)) } as *mut u8;
    unsafe {
        // Header verbatim (refcount + type_tag + flags) — same 8 bytes.
        core::ptr::copy_nonoverlapping(old as *const u8, p, 8);
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = 0; // rebuilt below
        *(p.add(DYNOBJ_CAP_OFF) as *mut u32) = new_cap;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_ENTRIES_CAP_OFF) as *mut u32) = entries_cap_for(new_cap);
        core::ptr::write_bytes(p.add(DYNOBJ_INDEX_OFF), 0xFF, new_cap as usize * 4);
    }
    let new_obj = p as *mut c_void;
    let new_ent = unsafe { entries(new_obj) };
    let new_idx = unsafe { index_ptr(new_obj) };
    let mut n: u32 = 0;
    for i in 0..old_len as usize {
        let kp_tagged = unsafe { (*old_ent.add(i)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        // Live keys are distinct, so the probe lands on an empty slot
        // (found = false, slot = fresh). Bitwise entry copy preserves
        // both the tagged key (ptr | flags) and the NaN-box value —
        // ownership moves, no rc traffic.
        let pr = unsafe { probe(new_obj, bucket_key_ptr(kp_tagged) as *const c_void) };
        unsafe {
            *new_ent.add(n as usize) = Entry {
                key_ptr_tagged: kp_tagged,
                value_anyv: (*old_ent.add(i)).value_anyv,
            };
            *new_idx.add(pr.slot as usize) = n;
        }
        n += 1;
    }
    unsafe {
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = n;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = n;
        *obj_slot = new_obj;
        free(old, block_bytes(old_cap));
    }
}
