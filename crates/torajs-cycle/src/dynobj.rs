//! DynObj mirror for the trial-deletion walk (RFC
//! 20260717-cycle-walk-dynobj blade 2) — the `arr.rs` twin for the
//! compact insertion-ordered dict.
//!
//! Layout mirror of `torajs-dynobj/src/layout.rs` (two blocks since
//! RFC 20260809-dynobj-store-split — a 32-byte address-stable header
//! cell plus an independent index+entries store the header points at;
//! resize swaps the store, never the header):
//!
//! ```text
//! header cell (32B)
//! offset | field
//! -------|------
//!   0    | universal heap header (8B)
//!   8    | count (u32)
//!  12    | cap (u32) — hash-index slot count
//!  16    | entries_len (u32) — dense-array used length (holes included)
//!  20    | entries_cap (u32)
//!  24    | store (*mut u8) — the index+entries block
//!
//! store block (store_bytes(cap))
//! offset | field
//! -------|------
//!   0    | index[cap] (4B each)
//!  4×cap | entries[] — { key_ptr_tagged: u64, value_anyv: u64 }
//! ```
//!
//! A deleted entry is a hole: `key_ptr_tagged == 0` (skipped by
//! iteration / drop / this walk). `value_anyv` is a NaN-box — the
//! cell-like gate filters immediates per slot, exactly like the
//! Arr<Any> walk.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;

/// Header-cell allocation size (heap header 8 + 4 × u32 + store ptr 8).
pub const DYNOBJ_HEADER_BYTES: usize = 32;
/// `store: *mut u8` offset within the header cell.
const DYNOBJ_STORE_OFF: usize = 24;
/// Per-entry stride: `key_ptr_tagged: u64` + `value_anyv: u64`.
const DYNOBJ_ENTRY_SIZE: usize = 16;
/// `cap: u32` offset (hash-index slot count).
const DYNOBJ_CAP_OFF: usize = 12;
/// `entries_len: u32` offset (dense-array used length).
const DYNOBJ_ENTRIES_LEN_OFF: usize = 16;
/// Mask isolating the real key Str pointer in a `key_ptr_tagged`
/// word (clears the low-3 W/E/C descriptor flag bits).
const BUCKET_KEY_PTR_MASK: u64 = !0x7u64;

/// Dense-entry iteration upper bound (holes included).
///
/// # Safety
/// `p` must be a live DynObj heap pointer.
#[inline]
pub unsafe fn dynobj_entries_len(p: *mut c_void) -> u64 {
    unsafe { *((p as *const u8).add(DYNOBJ_ENTRIES_LEN_OFF) as *const u32) as u64 }
}

/// The header cell's store pointer (the index+entries block).
///
/// # Safety
/// `p` must be a live DynObj header cell.
#[inline]
pub unsafe fn dynobj_store_ptr(p: *mut c_void) -> *mut c_void {
    unsafe { *((p as *const u8).add(DYNOBJ_STORE_OFF) as *const *mut c_void) }
}

/// Address of entry `i`'s 16-byte record (inside the store).
///
/// # Safety
/// `p` live DynObj; `i < dynobj_entries_len(p)`.
#[inline]
unsafe fn entry_at(p: *mut c_void, i: u64) -> *mut u8 {
    let cap = unsafe { *((p as *const u8).add(DYNOBJ_CAP_OFF) as *const u32) } as usize;
    unsafe { (dynobj_store_ptr(p) as *mut u8).add(4 * cap + i as usize * DYNOBJ_ENTRY_SIZE) }
}

/// Entry `i`'s value as a walkable-child candidate: NULL for a hole
/// or a non-cell NaN-box immediate, the heap pointer otherwise.
///
/// # Safety
/// `p` live DynObj; `i < dynobj_entries_len(p)`.
pub unsafe fn dynobj_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let e = unsafe { entry_at(p, i) };
    let key = unsafe { *(e as *const u64) };
    if key == 0 {
        return core::ptr::null_mut();
    }
    let v = unsafe { *(e.add(8) as *const u64) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero entry `i`'s value slot (collect_white's first-sweep cycle
/// break — a zero NaN-box reads as no-child through the cell-like
/// gate; the block is freed by the same collect frame, so the dict
/// invariants don't outlive the sweep).
///
/// # Safety
/// Same as [`dynobj_child_at`].
pub unsafe fn dynobj_value_slot_clear(p: *mut c_void, i: u64) {
    let e = unsafe { entry_at(p, i) };
    unsafe { *(e.add(8) as *mut u64) = 0 };
}

/// Entry `i`'s key Str pointer (flag bits stripped), or NULL for a
/// hole — collect_white's teardown drops each live key.
///
/// # Safety
/// Same as [`dynobj_child_at`].
pub unsafe fn dynobj_key_at(p: *mut c_void, i: u64) -> *mut c_void {
    let e = unsafe { entry_at(p, i) };
    let tagged = unsafe { *(e as *const u64) };
    (tagged & BUCKET_KEY_PTR_MASK) as *mut c_void
}

/// The store block's byte size (`index[cap]` + `entries[entries_cap]`)
/// — `torajs-dynobj::layout::store_bytes` mirror. Both blocks are
/// SIZED `__torajs_calloc` allocations with no libc-shim size header,
/// so collect_white must release each through the sized
/// `__torajs_free` (store first, then the [`DYNOBJ_HEADER_BYTES`]
/// header cell), not `__torajs_libc_free` (which reads a size word at
/// `p - 8` that a sized allocation never wrote).
///
/// # Safety
/// `p` must be a live DynObj header cell.
pub unsafe fn dynobj_store_bytes(p: *mut c_void) -> usize {
    let cap = unsafe { *((p as *const u8).add(DYNOBJ_CAP_OFF) as *const u32) };
    let entries_cap = (cap - cap / 8) as usize;
    cap as usize * 4 + entries_cap * DYNOBJ_ENTRY_SIZE
}
