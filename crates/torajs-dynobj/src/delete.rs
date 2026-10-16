//! DynObj key deletion.
//!
//! Spec §10.1.10 OrdinaryDelete. Drops the entry's owning key cell
//! and any ANY_HEAP value, turns the dense entry into a hole
//! (`key_ptr_tagged = 0`, skipped by iteration / drop / compact) and
//! the index slot into [`IDX_TOMBSTONE`] (probe walks past), then
//! decrements `count`. The hole is reclaimed when the dense array
//! fills and [`crate::resize`] compacts; re-inserting the key later
//! appends at the tail — JS "delete then set moves to the end" order.

use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::layout::{DYNOBJ_KEY_HOLE, IDX_TOMBSTONE};
use crate::probe::{bucket_key_ptr, count, drop_key, entries, index_ptr, probe, set_count};

/// Heap-header `flags` bit (u16 @6) raised on the %Object.prototype%
/// singleton when a key is deleted from it. The typed `in` reads it beside the singleton's `count`: one that holds
/// as many entries as its mint left ([`OBJECT_PROTO_MINT_ENTRIES`]) and
/// never lost one holds those entries alone. Bit 7 is free on DynObj:
/// torajs-rc's table gives it to Obj (ERROR), Arr (LENGTH_RO) and
/// Closure (FN_ASYNC), disjoint by tag.
pub const DYNOBJ_HDR_FLAG_KEY_DELETED: u16 = 1 << 7;

/// The entries %Object.prototype% holds once minted: the Annex B
/// `__proto__` accessor torajs-meta's `__torajs_object_proto_install`
/// defines, and nothing else.
pub const OBJECT_PROTO_MINT_ENTRIES: u32 = 1;

/// Mirror of `torajs_rc::builtin_proto::OBJECT_PROTO_TAG`.
const OBJECT_PROTO_TAG: usize = 1;

unsafe extern "C" {
    /// Cross-tier — universal heap-value drop (NaN-box-safe; the 7d-A
    /// cell gate filters immediate AnyValues so this is unconditional).
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// torajs-rc — the builtin prototype singletons by tag, 0 until
    /// minted.
    static __torajs_builtin_proto_slots: [AtomicUsize; OBJECT_PROTO_TAG + 1];
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
    // before the probe: deleting a builtin prototype's interned method
    // finds no entry, and neither does `Object.setPrototypeOf(p, null)`
    // on a prototype with no link entry
    unsafe { crate::proto_note::note_delete(obj) };
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
        // %Object.prototype%'s count answers for its key set only while
        // no key ever left it
        if obj as usize == __torajs_builtin_proto_slots[OBJECT_PROTO_TAG].load(Ordering::Acquire) {
            let flags = (obj as *mut u8).add(6) as *mut u16;
            *flags |= DYNOBJ_HDR_FLAG_KEY_DELETED;
        }
    }
    1
}
