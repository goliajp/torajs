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

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

/// 16-byte slot stride for Array<Any> — mirror of C macro
/// `__TORAJS_ANY_SLOT_BYTES` (the last C-side user, kept around
/// alongside the slot helpers, is removed by this same commit).
const ANY_SLOT_BYTES: usize = 16;

/// Tag value for ANY_HEAP slot (mirrors `any.rs`'s `ANY_HEAP`).
const ANY_HEAP: u64 = 4;

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
            __torajs_arrprops_drop_entry(p);
            __torajs_arr_free(p);
        }
    }
}

/// rc-aware drop for `Array<Any>` — walks every 16-byte slot, releases
/// each ANY_HEAP child's heap value, then frees the outer block.
///
/// Port of `runtime_str.c::__torajs_arr_drop_any` (P4.1-e, 2026-05-23).
/// Same NULL/STATIC_LITERAL/rc_dec gates as [`__torajs_arr_drop`], plus
/// the per-slot heap-child walker. Array<Any> bypasses the regular
/// cap-matched pool (different stride) → libc `free` direct rather
/// than [`__torajs_arr_free`] (which would route a 16-byte-stride
/// block into the 8-byte-stride pool and corrupt subsequent pulls).
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
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    if unsafe { __torajs_rc_dec(arr) } == 0 {
        // Shared — at least one other owner remains; keep alive.
        return;
    }
    // Last owner: walk slots, drop ANY_HEAP children, then free.
    unsafe {
        let arr_u8 = arr as *mut u8;
        let len = *(arr_u8.add(ARR_LEN_OFF) as *const u64);
        let slots = arr_u8.add(ARR_SLOTS_OFF);
        for i in 0..len {
            let off = (i as usize) * ANY_SLOT_BYTES;
            let tag = *(slots.add(off) as *const u64);
            if tag == ANY_HEAP {
                let val = *(slots.add(off + 8) as *const u64);
                __torajs_value_drop_heap(val as *mut c_void);
            }
        }
        __torajs_arrprops_drop_entry(arr);
        free(arr);
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
}
