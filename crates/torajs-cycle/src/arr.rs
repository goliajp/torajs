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
//!   +24 : props_dynobj (u64) — inline exec-props slot (Round 4 5a)
//!   +32 : slot data (N × 8 bytes)
//! ```
//!
//! Logical slot `i` lives at physical byte offset
//! `32 + (head + i) * 8`. (RFC 20260706 chunk 574 fixed a layout
//! drift here: Round 4 chunk 5a inserted the +24 props slot but
//! this mirror kept reading data at +24 — the walk saw the props
//! pointer as slot 0 and missed the true last slot. The payload-
//! only fixtures of chunk 534 never noticed.)
//!
//! Array<Any> uses the same 8-byte slot stride, holding NaN-box
//! `AnyValue`s (a cell encodes as the raw pointer, immediates
//! carry nonzero tag bits) — [`arr_child_at`] gates each slot
//! through the NaN-box cell predicate so both array families share
//! one walk path (RFC 20260706 Phase C).

use core::ffi::c_void;

/// Byte offset of `len` (u64) inside an Array<T> heap block.
pub const ARR_LEN_OFF: usize = 8;

/// Byte offset of `head` (u32) inside an Array<T> heap block.
pub const ARR_HEAD_OFF: usize = 20;

/// Byte offset of slot[0] *before head adjustment* — physical slot
/// `i` lives at `ARR_DATA_OFF + i * ARR_SLOT_STRIDE`. Mirrors
/// torajs-arr `ARR_SLOTS_OFF` (header 8 + len 8 + cap/head 8 +
/// props_dynobj 8).
pub const ARR_DATA_OFF: usize = 32;

/// Bytes per slot — shared by typed Array<T> (raw pointers) and
/// Array<Any> (NaN-box `AnyValue`s).
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

/// NaN-box discriminator mirror (torajs-anyvalue `nanbox.rs` —
/// layout-consumer copy, same pattern as the header constants):
/// a slot u64 encodes a heap cell iff the top 16 bits are 0, the
/// sentinel bit (`TAG_BIT_TYPE_OTHER`, bit 1) is clear, and the
/// value is nonzero. Int32/double/ShortStr immediates all carry
/// nonzero top-16 tags; Null/Undefined/Bool carry bit 1.
const NANBOX_TOP16_MASK: u64 = 0xFFFF_0000_0000_0000;
const NANBOX_TAG_BIT_TYPE_OTHER: u64 = 0x0000_0000_0000_0002;

/// Read logical slot `i` as a heap-cell pointer, or NULL when the
/// slot holds a NaN-box immediate (RFC 20260706 Phase C — Arr<Any>
/// slots are 8-byte NaN-box `AnyValue`s; a cell encodes as the raw
/// pointer). For `ARR_KIND_HEAP` arrays the predicate is equivalent
/// to the plain NULL check (malloc'd pointers have zero top-16 bits
/// and clear bit 1), so every walk phase uses this one accessor.
///
/// # Safety
/// Same as `arr_slot_at`.
#[inline]
pub unsafe fn arr_child_at(p: *mut c_void, i: u64) -> *mut c_void {
    let bits = unsafe { arr_slot_at(p, i) } as u64;
    if bits & NANBOX_TOP16_MASK == 0 && bits & NANBOX_TAG_BIT_TYPE_OTHER == 0 {
        bits as *mut c_void
    } else {
        core::ptr::null_mut()
    }
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
