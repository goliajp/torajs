//! DynObj mirror for the trial-deletion walk (RFC
//! 20260717-cycle-walk-dynobj blade 2) — the `arr.rs` twin for the
//! compact insertion-ordered dict.
//!
//! Layout mirror of `torajs-dynobj/src/layout.rs` (single heap
//! block):
//!
//! ```text
//! offset      | field
//! ------------|------
//!   0         | universal heap header (8B)
//!   8         | count (u32)
//!  12         | cap (u32) — hash-index slot count
//!  16         | entries_len (u32) — dense-array used length (holes included)
//!  20         | entries_cap (u32)
//!  24         | index[cap] (4B each)
//!  24 + 4×cap | entries[] — { key_ptr_tagged: u64, value_anyv: u64 }
//! ```
//!
//! A deleted entry is a hole: `key_ptr_tagged == 0` (skipped by
//! iteration / drop / this walk). `value_anyv` is a NaN-box — the
//! cell-like gate filters immediates per slot, exactly like the
//! Arr<Any> walk.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;

/// Header bytes before `index[]` (heap header 8 + 4 × u32).
const DYNOBJ_HDR_SIZE: usize = 24;
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

/// Address of entry `i`'s 16-byte record.
///
/// # Safety
/// `p` live DynObj; `i < dynobj_entries_len(p)`.
#[inline]
unsafe fn entry_at(p: *mut c_void, i: u64) -> *mut u8 {
    let cap = unsafe { *((p as *const u8).add(DYNOBJ_CAP_OFF) as *const u32) } as usize;
    unsafe { (p as *mut u8).add(DYNOBJ_HDR_SIZE + 4 * cap + i as usize * DYNOBJ_ENTRY_SIZE) }
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

/// Total heap-block byte size (header + `index[cap]` +
/// `entries[entries_cap]`) — `torajs-dynobj::layout::block_bytes`
/// mirror. The dict is a SIZED `__torajs_calloc` allocation with no
/// libc-shim size header, so collect_white must release it through
/// the sized `__torajs_free`, not `__torajs_libc_free` (which reads
/// a size word at `p - 8` that a sized allocation never wrote).
///
/// # Safety
/// `p` must be a live DynObj heap pointer.
pub unsafe fn dynobj_block_bytes(p: *mut c_void) -> usize {
    let cap = unsafe { *((p as *const u8).add(DYNOBJ_CAP_OFF) as *const u32) };
    let entries_cap = (cap - cap / 8) as usize;
    DYNOBJ_HDR_SIZE + cap as usize * 4 + entries_cap * DYNOBJ_ENTRY_SIZE
}
