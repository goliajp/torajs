//! `arr.slice(start, end)` — fresh array containing the [start, end)
//! range.
//!
//! Port of `runtime_str.c::__torajs_arr_slice` (P4.1-f, 2026-05-23).
//!
//! - Negative indices: `start < 0 → max(len + start, 0)`; same for end.
//!   Per ES spec §22.1.3.25. Empty range when `hi < lo`.
//! - Element-type-agnostic — operates on the 8-byte-slot layout
//!   (`Array<T>`); `Array<Any>` 16-byte-slot path is a separate
//!   `arr_slice_any` if/when needed.
//! - Single malloc + one memcpy. Returns a fresh `+1`-rc heap pointer.
//! - T-13.5 deque-aware: source's head_offset is folded into the
//!   memcpy source pointer via `data_ptr_at`.

use core::ffi::c_void;

use torajs_rc::{ARR_ELEM_KIND_MASK, ARR_KIND_HEAP, FLAG_ARR_ANY, HeapHeader};

use crate::layout::{ARR_LEN_OFF, ARR_PROPS_OFF, ARR_SLOTS_OFF, TAG_ARR};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat malloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump (no-ops
    /// for non-cell bit patterns and NULL).
    fn __torajs_rc_inc(p: *mut c_void);
}

const ARR_CAP_LOW32_OFF: usize = 16;
const ARR_HEAD_OFF: usize = 20;
/// Array<Any> NaN-box slot stride — one u64 AnyValue per slot
/// (mirrors `any.rs` / `drop.rs` / `iter.rs`).
const ANY_SLOT_BYTES: usize = 8;

/// Pointer to logical slot `i` of `arr`. Folds T-13.5 `head_offset`
/// into the address so the caller sees a contiguous logical view.
#[inline]
unsafe fn data_ptr_at(arr: *const u8, i: usize) -> *const u8 {
    unsafe {
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as usize;
        arr.add(ARR_SLOTS_OFF + (head + i) * 8)
    }
}

/// Internal alloc + header init for `Array<T>` (matches C's
/// `arr_alloc_`). Bypasses the cap-matched pool — slice always
/// produces a fresh right-sized block (no cap slack), so pooling
/// would just waste a search for nothing.
#[inline]
unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_SLOTS_OFF + (cap as usize) * 8;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0; // flags
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        // Round 4 chunk 5a props_dynobj slot — NULL like alloc.rs
        // (was malloc garbage here; a props consumer's NULL gate
        // would chase it).
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        p
    }
}

/// `arr.slice(start, end)` for regular `Array<T>` (8-byte slots).
///
/// # Safety
/// `arr` must be a valid Array<T> heap block pointer (NOT Array<Any> —
/// stride mismatch). Returned pointer is owned (+1 rc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_slice(arr: *const u8, start: i64, end: i64) -> *mut u8 {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let (lo, hi) = clamp_range(len as i64, start, end);
        let out_len = (hi - lo) as u64;
        let p = arr_alloc_fresh(out_len, out_len);
        if out_len > 0 {
            let src = data_ptr_at(arr, lo as usize);
            let dst = p.add(ARR_SLOTS_OFF);
            core::ptr::copy_nonoverlapping(src, dst, (out_len as usize) * 8);
        }
        p
    }
}

/// ES spec §22.1.3.25 negative-index clamp shared by both slice
/// kernels: `start < 0 → max(len + start, 0)`, same for end; empty
/// range (`hi = lo`) when they cross.
#[inline]
fn clamp_range(ilen: i64, start: i64, end: i64) -> (i64, i64) {
    let lo = if start < 0 {
        if start + ilen < 0 { 0 } else { start + ilen }
    } else if start > ilen {
        ilen
    } else {
        start
    };
    let mut hi = if end < 0 {
        if end + ilen < 0 { 0 } else { end + ilen }
    } else if end > ilen {
        ilen
    } else {
        end
    };
    if hi < lo {
        hi = lo;
    }
    (lo, hi)
}

/// `arr.slice(start, end)` where the receiver arrived through `any`
/// (any-method-call RFC C4+) — kind-aware over the block's own
/// self-description:
///
/// - `FLAG_ARR_ANY` (16-byte NaN-box slots) → per-slot copy +
///   NaN-box-safe `rc_inc` (each new slot takes an independent ref,
///   the source keeps its own; same ledger as `arr_extend_any`).
/// - typed 8-byte slots → single memcpy (deque head folded), then
///   copy the 3-bit elem-kind field onto the fresh header so the
///   product stays self-describing; `ARR_KIND_HEAP` slots rc_inc
///   per pointer. `ARR_KIND_UNSET` keeps the raw-memcpy pre-RFC
///   fallback (scalar semantics, matching index-get / print).
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer. Returned pointer is a
/// fresh owned (+1 rc) array of the same slot layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_slice(arr: *const u8, start: i64, end: i64) -> *mut u8 {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let (lo, hi) = clamp_range(len as i64, start, end);
        let out_len = (hi - lo) as u64;
        let flags = (*(arr as *const HeapHeader)).flags;
        if flags & FLAG_ARR_ANY != 0 {
            // Any-arrays never deque-shift — head is always 0.
            let total = ARR_SLOTS_OFF + (out_len as usize) * ANY_SLOT_BYTES;
            let p = malloc(total) as *mut u8;
            *(p as *mut u32) = 1; // refcount
            *(p.add(4) as *mut u16) = TAG_ARR;
            *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
            *(p.add(ARR_LEN_OFF) as *mut u64) = out_len;
            *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = out_len as u32;
            *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
            *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
            for i in 0..out_len as usize {
                let av =
                    *(arr.add(ARR_SLOTS_OFF + (lo as usize + i) * ANY_SLOT_BYTES) as *const u64);
                __torajs_rc_inc(av as *mut c_void);
                *(p.add(ARR_SLOTS_OFF + i * ANY_SLOT_BYTES) as *mut u64) = av;
            }
            return p;
        }
        let p = __torajs_arr_slice(arr, start, end);
        let kind_bits = flags & ARR_ELEM_KIND_MASK;
        *(p.add(6) as *mut u16) |= kind_bits;
        if kind_bits >> torajs_rc::ARR_ELEM_KIND_SHIFT == ARR_KIND_HEAP {
            for i in 0..out_len as usize {
                let cell = *(p.add(ARR_SLOTS_OFF + i * 8) as *const u64);
                __torajs_rc_inc(cell as *mut c_void);
            }
        }
        p
    }
}
