//! DynObj table resize — store swap: fresh store + compact + index
//! rebuild, header cell untouched (RFC 20260809-dynobj-store-split).
//!
//! Private-to-crate helper used by [`crate::set`] / [`crate::define`]
//! when the dense entry array fills (`entries_len == entries_cap`).
//!
//! The header cell's address never changes — every owner (class-proto
//! globals, classmeta registry, instance chains, aliased variables,
//! the cycle collector's root buffer) stays valid across growth. The
//! predecessor single-block layout relocated the whole object here and
//! wrote the new address back through a single caller slot, leaving
//! every other owner on a freed block (multi-owner identity split /
//! UAF). CPython's `dictresize` swaps `ma_keys` inside a stable
//! `PyDictObject` for exactly this reason; this is that shape.
//!
//! Algorithm:
//! 1. Pick the smallest power-of-2 cap with `cap > count * 3` (at
//!    least [`DYNOBJ_INITIAL_CAP`]). When most entries are holes this
//!    lands on the same or a smaller cap — the pass degenerates to a
//!    pure compact; when the table is genuinely full it doubles+.
//! 2. calloc a fresh store, fill its index with IDX_EMPTY, and swap it
//!    into the header (cap / entries_cap / count / entries_len reset
//!    with it — the old store's geometry lives on in locals).
//! 3. Walk the old dense array in order; skip holes; append each live
//!    entry to the new dense array (insertion order preserved) and
//!    probe the new index for its slot (all distinct keys ⇒ lands on
//!    an empty slot). Tombstones and holes vanish.
//! 4. Free the old store. (No pool interaction — the pool recycles
//!    header+store pairs, and a swapped-out store is a bare block.)

use core::ffi::c_void;

use crate::layout::{
    DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF, DYNOBJ_ENTRIES_CAP_OFF, DYNOBJ_ENTRIES_LEN_OFF,
    DYNOBJ_INITIAL_CAP, DYNOBJ_KEY_HOLE, entries_cap_for, store_bytes,
};
use crate::probe::{
    Entry, bucket_key_ptr, cap, count, entries, entries_len, index_ptr, probe, set_store_ptr,
    store_ptr,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat calloc/free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
}

/// Swap `obj`'s store for a fresh one sized for its live count,
/// compacting holes out of the dense array and rebuilding the hash
/// index. The old store is freed; the header cell (and therefore the
/// object's identity) is untouched. Guarantees at least one free
/// dense slot on return.
///
/// # Safety
/// `obj` must be a live dynobj header cell with a live store.
pub(crate) unsafe fn resize(obj: *mut c_void) {
    let old_store = unsafe { store_ptr(obj) };
    let old_cap = unsafe { cap(obj) };
    let old_len = unsafe { entries_len(obj) };
    let old_ent = unsafe { old_store.add(old_cap as usize * 4) } as *mut Entry;
    let live = unsafe { count(obj) };

    // CPython growth rule: smallest power-of-2 cap > count * 3. The
    // 7/8 dense ratio then gives entries_cap > count, so the pending
    // insert always fits (debug-asserted below).
    let mut new_cap = DYNOBJ_INITIAL_CAP;
    while (new_cap as u64) <= (live as u64) * 3 {
        new_cap *= 2;
    }
    debug_assert!(entries_cap_for(new_cap) > live);

    let new_store = unsafe { calloc(store_bytes(new_cap)) } as *mut u8;
    unsafe {
        core::ptr::write_bytes(new_store, 0xFF, new_cap as usize * 4);
        // Swap the store in and reset the geometry — the re-insert
        // walk below reads the header through the normal accessors,
        // so cap/store must describe the NEW block before probing.
        let p = obj as *mut u8;
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = 0; // rebuilt below
        *(p.add(DYNOBJ_CAP_OFF) as *mut u32) = new_cap;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = 0;
        *(p.add(DYNOBJ_ENTRIES_CAP_OFF) as *mut u32) = entries_cap_for(new_cap);
        set_store_ptr(obj, new_store);
    }
    let new_ent = unsafe { entries(obj) };
    let new_idx = unsafe { index_ptr(obj) };
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
        let pr = unsafe { probe(obj, bucket_key_ptr(kp_tagged) as *const c_void) };
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
        let p = obj as *mut u8;
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = n;
        *(p.add(DYNOBJ_ENTRIES_LEN_OFF) as *mut u32) = n;
        free(old_store as *mut c_void, store_bytes(old_cap));
    }
}
