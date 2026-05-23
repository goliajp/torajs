//! Array fast-path ops — push_unchecked + extend_unchecked.
//!
//! Port of `ssa_inkwell::define_arr_push_unchecked` +
//! `runtime_str.c::__torajs_arr_extend_unchecked` (P4.1-c, 2026-05-23).
//!
//! Both operate on 8-byte-slot `Array<T>` (NOT `Array<Any>` which has
//! 16-byte slots). Caller MUST have pre-sized cap (see callsite
//! contract: typically a paired `arr_reserve(arr, len + n)` upstream).
//!
//! T-13.5 deque-aware: logical slot[i] resolves to
//! `data_ptr + (head_offset + i) * 8` so callers can use either a
//! freshly-allocated head=0 array or a shifted deque without special-
//! casing in the codegen call site.
//!
//! arr_push (the cap-check + grow variant) is still emitted by inkwell
//! as `define_arr_push` since the realloc path is non-trivial and
//! benefits from LLVM's pure-IR inlining + alias analysis. Port deferred
//! to P4.1-d alongside `arr_reserve` and `arr_shift`.

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

/// Cap slot (low 32 bits) + head_offset (high 32) live at offset 16.
const ARR_HEAD_OFF: usize = 20;

/// Pointer to logical slot 0 — folds head_offset into the math so
/// callers see a contiguous logical array regardless of T-13.5 deque
/// shift state.
#[inline]
unsafe fn data_ptr(arr: *mut u8) -> *mut u8 {
    unsafe {
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as usize;
        arr.add(ARR_SLOTS_OFF + head * 8)
    }
}

/// `__torajs_arr_push_unchecked(arr, val)` — append `val` to a regular
/// `Array<T>`. Caller asserts `cap >= len + 1`; UB if violated. Used
/// after a one-shot `arr_reserve` so per-push cap check is gone.
///
/// # Safety
/// `arr` must be a valid 8-byte-slot Array heap block with sufficient
/// capacity for one more element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_unchecked(arr: *mut c_void, val: i64) {
    let arr = arr as *mut u8;
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let slot = data_ptr(arr).add(len as usize * 8) as *mut i64;
        *slot = val;
        *(arr.add(ARR_LEN_OFF) as *mut u64) = len + 1;
    }
}

/// `__torajs_arr_extend_unchecked(dst, src)` — append every element of
/// `src` to `dst` via a single memcpy. Caller asserts `dst.cap >=
/// dst.len + src.len`; UB if violated. Typical caller is a literal-
/// with-spreads materializer that pre-computes total length and allocs
/// once.
///
/// # Safety
/// Both `dst` and `src` must be valid 8-byte-slot Array heap blocks;
/// `dst` must have capacity for the extra `src.len` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_extend_unchecked(dst: *mut u8, src: *const u8) {
    unsafe {
        let dst_len = *(dst.add(ARR_LEN_OFF) as *const u64);
        let src_len = *(src.add(ARR_LEN_OFF) as *const u64);
        if src_len == 0 {
            return;
        }
        let dst_slot = data_ptr(dst).add(dst_len as usize * 8);
        // src side is read-only; cast through *const u8 + const data_ptr
        // semantics. Implementation mirrors the C version's
        // memcpy(ARR_SLOT(dst, dst_len), ARR_CSLOT(src, 0), src_len * 8).
        let src_head = *(src.add(ARR_HEAD_OFF) as *const u32) as usize;
        let src_slot = src.add(ARR_SLOTS_OFF + src_head * 8);
        core::ptr::copy_nonoverlapping(src_slot, dst_slot, src_len as usize * 8);
        *(dst.add(ARR_LEN_OFF) as *mut u64) = dst_len + src_len;
    }
}
