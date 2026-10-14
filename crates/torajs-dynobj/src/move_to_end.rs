//! Move one own entry to the end of the insertion order.
//!
//! §10.1.11 OrdinaryOwnPropertyKeys reports string keys in insertion
//! order, and a dense entry can only be APPENDED — so a define that
//! has to land BEHIND an entry already in the table has no way to say
//! so. The answer JS itself gives is "delete then set moves it to the
//! end": what cannot move backwards can be moved forwards past.
//!
//! Doing that through the public delete + define pair would drop the
//! entry's value and mint a replacement, so the attributes and the
//! cell identity would have to be re-derived by the caller. This
//! moves the entry itself: the key cell's reference, the boxed value
//! and the attribute bits are carried over verbatim and the vacated
//! slot becomes a hole, which is exactly what a delete leaves behind
//! (`resize` compacts it away on the next growth).

use core::ffi::c_void;

use crate::layout::{DYNOBJ_KEY_HOLE, IDX_TOMBSTONE, TAG_DYNOBJ};
use crate::probe::{
    Entry, count, entries, entries_cap, entries_len, index_ptr, probe, set_count, set_entries_len,
};

/// `__torajs_dynobj_move_own_to_end(obj, key)` — re-append `key`'s
/// own entry so it sorts after every other string key. Answers 1 iff
/// an entry moved (0 for a NULL / non-dynobj receiver, an absent key,
/// or a key that is already last — nothing to do).
///
/// The move is unobservable except in own-key order: same key cell,
/// same value, same attributes, and no drop / mint in between, so a
/// non-configurable or accessor entry rides across unchanged.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header;
/// `key` (if reached) is a live Str / Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_move_own_to_end(
    obj: *mut c_void,
    key: *const c_void,
) -> i32 {
    if obj.is_null() {
        return 0;
    }
    if unsafe { crate::get::type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    unsafe {
        let pr = probe(obj, key);
        if !pr.found || pr.entry + 1 == entries_len(obj) {
            return 0;
        }
        // Carry the whole entry out, then vacate its slot WITHOUT the
        // delete kernel's drops — the key reference and the value's
        // stake are transferred, not released and retaken.
        let e = entries(obj).add(pr.entry as usize);
        let carried_key = (*e).key_ptr_tagged;
        let carried_value = (*e).value_anyv;
        (*e).key_ptr_tagged = DYNOBJ_KEY_HOLE;
        (*e).value_anyv = 0;
        *index_ptr(obj).add(pr.slot as usize) = IDX_TOMBSTONE;
        set_count(obj, count(obj) - 1);
        // The dense array needs a free tail slot. `resize` swaps the
        // store inside a header cell whose address never changes, so
        // every owner of `obj` stays valid; it also compacts the hole
        // just made and rebuilds the index, hence the re-probe.
        if entries_len(obj) == entries_cap(obj) {
            crate::resize::resize(obj);
        }
        // The key is absent now, so this answers an insertion slot
        // (the tombstone above, or an earlier one on the same chain).
        let slot = probe(obj, key).slot;
        let e_idx = entries_len(obj);
        *entries(obj).add(e_idx as usize) = Entry {
            key_ptr_tagged: carried_key,
            value_anyv: carried_value,
        };
        *index_ptr(obj).add(slot as usize) = e_idx;
        set_entries_len(obj, e_idx + 1);
        set_count(obj, count(obj) + 1);
    }
    1
}
