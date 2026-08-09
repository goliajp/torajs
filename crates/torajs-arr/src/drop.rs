//! `__torajs_arr_drop` — rc-aware drop for Array heap blocks.
//!
//! Port of `ssa_inkwell::define_arr_drop` (IR-emitted before P4.1-a;
//! now a Rust extern with identical semantics).
//!
//! Semantics (mirror IR shape 1:1):
//! 1. NULL → no-op
//! 2. Flags has `FLAG_STATIC_LITERAL` → no-op (`.rodata`-baked arrays
//!    don't have a refcount we own)
//! 3. `__torajs_rc_dec(p)` → if last owner (returned 1):
//!    a. `__torajs_arrprops_drop_entry(p)` — release any associated
//!       key-value props (no-op for arrays that never had `arr.x = v`)
//!    b. `__torajs_arr_free(p)` — pool-aware free (LIFO pool for
//!       cap ≤ ARR_POOL_PAYLOAD, libc free otherwise)
//!
//! Caller (ssa_lower `emit_drop_value Type::Arr`) is responsible for
//! walking refcounted ELEMENT types FIRST (e.g. `Arr<Str>` walks each
//! Str's rc_dec before calling here). This fn only owns the array
//! header + the slots' backing storage.

use core::ffi::c_void;

use torajs_rc::{
    ARR_ELEM_KIND_MASK, ARR_ELEM_KIND_SHIFT, ARR_KIND_HEAP, FLAG_ARR_ANY, FLAG_BUFFERED,
    FLAG_STATIC_LITERAL, FLAG_SUBCLASSED, HeapHeader,
};

use crate::layout::{arr_data, arr_data_is_inline, arr_live_extent};

/// 8-byte slot stride for Array<Any> — Step 7e-A (NaN-box AnyValue
/// per slot; mirrors `any.rs`'s `ANY_SLOT_BYTES`).
const ANY_SLOT_BYTES: usize = 8;

unsafe extern "C" {
    /// Cross-tier — torajs-rc. Decrements rc; returns 1 if hit zero
    /// (caller takes ownership of the now-dangling pointer).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// Cross-tier — runtime_str.c's pool-aware array free. Returns
    /// blocks with `cap ≤ ARR_POOL_PAYLOAD` to a LIFO pool; libc free
    /// for the rest.
    fn __torajs_arr_free(p: *mut c_void);

    /// Cross-tier — runtime_str.c's array-prop side-table. Drops the
    /// per-array key-value entry if one exists. No-op for the common
    /// case (most arrays never had `arr.x = v` written).
    fn __torajs_arrprops_drop_entry(p: *mut c_void);

    /// Cross-tier — runtime_str.c's universal heap value dropper.
    /// Used by `__torajs_arr_drop_any` to release each ANY_HEAP slot's
    /// child value before freeing the outer block.
    fn __torajs_value_drop_heap(p: *mut c_void);

    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b cutover.
    /// Used by `arr_drop_any` directly (it allocates with
    /// `malloc`/`realloc` and bypasses the pool since Any-arrays'
    /// 16-byte stride doesn't match).
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// torajs-cycle — register a potential cycle root when an rc dec
    /// stays positive on a pointer-carrying array (chunk 614: the
    /// obj drop had this hook since T-26.C, the arr drops never did,
    /// so a pure-array cycle was never even a collection candidate).
    fn __torajs_cycle_buffer(p: *mut c_void);

    /// torajs-cycle — scrub a buffered block from the root buffer
    /// before its memory is freed / pooled (a stale entry would be
    /// walked by the next collect).
    fn __torajs_cycle_unbuffer(p: *mut c_void);

    /// torajs-meta — remove a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0). A stale entry would
    /// resurrect the class identity on the next cell minted at the
    /// same address, so every hit-zero path gates on
    /// `FLAG_SUBCLASSED` and scrubs.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
}

/// True iff the array's slots can carry heap-cell pointers — the only
/// shapes that can close a reference cycle (mirrors the arr half of
/// torajs-cycle `is_visitable_arr`). Scalar-kind / UNSET arrays are
/// leaves: skip the cycle-buffer call entirely on their hot drop path.
#[inline]
fn arr_may_cycle(flags: u16) -> bool {
    flags & FLAG_ARR_ANY != 0
        || (flags & ARR_ELEM_KIND_MASK) >> ARR_ELEM_KIND_SHIFT == ARR_KIND_HEAP
}

/// rc-aware drop. NULL-safe + `FLAG_STATIC_LITERAL`-safe.
///
/// # Safety
/// `p` is either NULL or a valid Array heap block pointer with a live
/// universal heap header at offset 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    // STATIC_LITERAL flag check — `.rodata` array literals must never
    // be rc-decremented (the store would page-fault) or freed.
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } != 0 {
        unsafe {
            // A previously-buffered block must leave the cycle root
            // buffer before arr_free can pool-recycle its cell.
            if header.flags & FLAG_BUFFERED != 0 {
                __torajs_cycle_unbuffer(p);
            }
            if header.flags & FLAG_SUBCLASSED != 0 {
                __torajs_subclass_drop_entry(p);
            }
            __torajs_arrprops_drop_entry(p);
            __torajs_arr_free(p);
        }
    } else if arr_may_cycle(header.flags) {
        // Still referenced — a pointer-carrying array is a potential
        // cycle root, exactly like the obj drop's T-26.C hook.
        unsafe { __torajs_cycle_buffer(p) };
    }
}

/// rc-aware drop for `Array<Any>` — walks every 8-byte AnyValue slot,
/// releases each cell-tagged heap value (via the NaN-box-safe
/// `value_drop_heap`), then frees the outer block.
///
/// Port of `runtime_str.c::__torajs_arr_drop_any` (P4.1-e, 2026-05-23).
/// Same NULL/STATIC_LITERAL/rc_dec gates as [`__torajs_arr_drop`], plus
/// the per-slot heap-child walker. Array<Any> bypasses the regular
/// cap-matched pool (different stride) → libc `free` direct rather
/// than [`__torajs_arr_free`] (which would route the Any-stride block
/// into the regular-stride pool and corrupt subsequent pulls).
///
/// arrprops side-table is checked too — most Any-arrays never write
/// `arr.x = v`, but the side-table drop is the same cheap no-op as
/// for regular arrays.
///
/// # Safety
/// `arr` is either NULL or a valid `Array<Any>` heap pointer with
/// `FLAG_ARR_ANY` set (caller's typecheck ensures it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_any(arr: *mut c_void) {
    if arr.is_null() {
        return;
    }
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_ARR_ANY == 0 {
        // RFC 20260707 chunk 623 — typed block behind a static
        // Arr<Any> slot (T-11 widen): the NaN-box slot walk below
        // misreads raw scalars (a small bit-1-clear i64 like 4
        // passes the cell predicate → deref + free, SIGSEGV), and
        // typed blocks pool-free via arr_free, not the libc bypass.
        // Route per elem kind: HEAP walks children (the only place
        // elements release when the last ref dies through the any
        // world), scalar frees straight. UNSET = unmarked scalar or
        // a missed coercion boundary — no walk keeps it a leak at
        // worst, never a bad deref.
        if header.arr_elem_kind() == ARR_KIND_HEAP {
            return unsafe { __torajs_arr_drop_heap(arr) };
        }
        return unsafe { __torajs_arr_drop(arr) };
    }
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(arr) } == 0 {
        // Shared — at least one other owner remains; an Any-array
        // holds cell pointers, so it's a potential cycle root.
        if arr_may_cycle(header.flags) {
            unsafe { __torajs_cycle_buffer(arr) };
        }
        return;
    }
    // Last owner: walk slots, drop each cell-tagged heap value via
    // the NaN-box-safe dispatcher (immediates skip), then free.
    unsafe {
        if header.flags & FLAG_BUFFERED != 0 {
            __torajs_cycle_unbuffer(arr);
        }
        let arr_u8 = arr as *mut u8;
        // Sparse tail (RFC 20260810-arr-sparse-grow) — only the
        // materialized extent holds slots; the implicit-hole tail has
        // no storage to walk.
        let len = arr_live_extent(arr_u8);
        let slots = arr_data(arr_u8);
        for i in 0..len {
            let off = (i as usize) * ANY_SLOT_BYTES;
            let av = *(slots.add(off) as *const u64);
            __torajs_value_drop_heap(av as *mut c_void);
        }
        if header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(arr);
        }
        __torajs_arrprops_drop_entry(arr);
        // B1 — a grown array spilled its slots to an independent
        // buffer; release it before the cell (mirror of
        // __torajs_arr_free's spill branch — this fn bypasses
        // arr_free for the stride reason above and missed the B1
        // retrofit: every grown Array<Any> leaked its buffer).
        if !arr_data_is_inline(arr_u8) {
            free(slots as *mut c_void);
        }
        free(arr);
    }
}

/// rc-aware drop for a typed array whose element kind is
/// `ARR_KIND_HEAP` (`Array<Str>` / `Array<Arr>` / any refcounted
/// cell slots) — Any-dynamic-access RFC (20260704) S6.
///
/// Same NULL/STATIC_LITERAL/rc_dec gates as [`__torajs_arr_drop`];
/// on hit-zero, walks every 8-byte cell-pointer slot through the
/// universal `value_drop_heap` dispatcher (releasing the array's
/// reference to each element), then frees through the regular
/// pool-aware [`__torajs_arr_free`] path (typed stride — unlike
/// `Array<Any>`'s libc-free bypass).
///
/// Caller is `__torajs_value_drop_heap`'s Tag::Arr arm when the
/// header's elem-kind field says HEAP: the typed drop site's static
/// element walk is rc==1-gated, so when the LAST reference is
/// released through the `any` world this walker is the only place
/// the elements get released.
///
/// Slots are read through the head-folded logical base (T-13.5
/// deque state safe).
///
/// # Safety
/// `arr` is either NULL or a valid typed Array heap pointer whose
/// slots are cell pointers (elem-kind HEAP; caller dispatches on
/// the header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_heap(arr: *mut c_void) {
    if arr.is_null() {
        return;
    }
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(arr) } == 0 {
        // Shared — heap-elem slots are cell pointers; potential
        // cycle root (same hook as `__torajs_arr_drop_any`).
        if arr_may_cycle(header.flags) {
            unsafe { __torajs_cycle_buffer(arr) };
        }
        return;
    }
    unsafe {
        if header.flags & FLAG_BUFFERED != 0 {
            __torajs_cycle_unbuffer(arr);
        }
        let arr_u8 = arr as *mut u8;
        // Sparse tail (RFC 20260810-arr-sparse-grow) — only the
        // materialized extent holds slots; the implicit-hole tail has
        // no storage to walk.
        let len = arr_live_extent(arr_u8);
        let slots = crate::ops::data_ptr(arr_u8);
        for i in 0..len {
            let child = *(slots.add(i as usize * 8) as *const *mut c_void);
            __torajs_value_drop_heap(child);
        }
        if header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(arr);
        }
        __torajs_arrprops_drop_entry(arr);
        __torajs_arr_free(arr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_null_is_noop() {
        unsafe { __torajs_arr_drop(core::ptr::null_mut()) };
    }

    #[test]
    fn drop_any_null_is_noop() {
        unsafe { __torajs_arr_drop_any(core::ptr::null_mut()) };
    }

    #[test]
    fn drop_heap_null_is_noop() {
        unsafe { __torajs_arr_drop_heap(core::ptr::null_mut()) };
    }
}
