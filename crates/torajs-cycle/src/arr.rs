//! Array-side heap-block accessors used by the cycle collector to
//! walk Array<T> children during mark/scan/collect phases.
//!
//! Mirrors the byte-offset accessors from `runtime_cycle.c`. Kept
//! independent of `torajs-arr` (no Cargo dep) — these are raw
//! byte-offset reads, not a struct mirror, to keep the cycle
//! collector purely a layout consumer (same pattern as the C
//! original).
//!
//! ## Array<T> layout (8-byte typed slots)
//!
//! ```text
//!   +0  : universal heap header (8B)
//!   +8  : len  (u64)
//!   +16 : cap  (u32)
//!   +20 : head (u32) — physical offset of logical[0] for deque shift
//!   +24 : slot data (N × 8 bytes)
//! ```
//!
//! Logical slot `i` lives at physical byte offset
//! `24 + (head + i) * 8`.
//!
//! Array<Any> uses a 16-byte slot stride (tag + payload pair) but
//! isn't on the cycle-collector hot path yet — only Array<T> with
//! 8-byte slots is exercised. When Array<Any> joins the cycle
//! walk, a sibling module would mirror this with stride = 16.

use core::ffi::c_void;

/// Byte offset of `len` (u64) inside an Array<T> heap block.
pub const ARR_LEN_OFF: usize = 8;

/// Byte offset of `head` (u32) inside an Array<T> heap block.
pub const ARR_HEAD_OFF: usize = 20;

/// Byte offset of slot[0] *before head adjustment* — physical slot
/// `i` lives at `ARR_DATA_OFF + i * ARR_SLOT_STRIDE`.
pub const ARR_DATA_OFF: usize = 24;

/// Bytes per slot for Array<T> (typed). Array<Any> uses 16 but
/// isn't walked here yet.
pub const ARR_SLOT_STRIDE: usize = 8;

/// Read the logical `len` of an Array<T> block.
///
/// # Safety
/// `p` must be a non-NULL Array<T> heap pointer (caller filtered
/// via `is_visitable_arr`).
#[inline]
pub unsafe fn arr_len_of(p: *mut c_void) -> u64 {
    unsafe { *((p as *const u8).add(ARR_LEN_OFF) as *const u64) }
}

/// Compute the byte offset of logical slot `i` in an Array<T>
/// block, applying the deque-shift `head` offset.
#[inline]
pub unsafe fn arr_slot_byte_off(p: *mut c_void, i: u64) -> usize {
    let head = unsafe { *((p as *const u8).add(ARR_HEAD_OFF) as *const u32) };
    ARR_DATA_OFF + ((head as u64 + i) as usize) * ARR_SLOT_STRIDE
}

/// Read logical slot `i` as a raw `*mut c_void`.
///
/// # Safety
/// Same as `arr_len_of`; additionally `i` must be < `arr_len_of(p)`.
#[inline]
pub unsafe fn arr_slot_at(p: *mut c_void, i: u64) -> *mut c_void {
    let off = unsafe { arr_slot_byte_off(p, i) };
    unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) }
}

/// Zero out logical slot `i`. Used by `collect_white` to break a
/// cycle before recursing so the recursive collect doesn't
/// re-decrement.
///
/// # Safety
/// Same as `arr_slot_at`.
#[inline]
pub unsafe fn arr_slot_clear(p: *mut c_void, i: u64) {
    let off = unsafe { arr_slot_byte_off(p, i) };
    unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) = core::ptr::null_mut() };
}
