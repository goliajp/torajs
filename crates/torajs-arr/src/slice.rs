//! `arr.slice(start, end)` — fresh array containing the [start, end)
//! range.
//!
//! Port of `runtime_str.c::__torajs_arr_slice` (P4.1-f, 2026-05-23).
//!
//! - Negative indices: `start < 0 → max(len + start, 0)`; same for end.
//!   Per ES spec §22.1.3.25. Empty range when `hi < lo`.
//! - Element-type-agnostic — operates on the 8-byte-slot layout
//!   (`Array<T>`); any-receiver dispatch (Array<Any> NaN-box slots +
//!   kind-aware typed slots) is `__torajs_arr_any_slice` below.
//! - Single malloc + one memcpy. Returns a fresh `+1`-rc heap pointer.
//! - T-13.5 deque-aware: source's head_offset is folded into the
//!   memcpy source pointer via `data_ptr_at`.

use core::ffi::c_void;

use torajs_rc::{ARR_ELEM_KIND_MASK, ARR_KIND_HEAP, FLAG_ARR_ANY, HeapHeader};

use crate::layout::{
    ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, ARR_PROPS_OFF, TAG_ARR, arr_data,
};

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
        arr_data(arr).add((head + i) * 8)
    }
}

/// Internal alloc + header init for `Array<T>` (matches C's
/// `arr_alloc_`). Bypasses the cap-matched pool — slice always
/// produces a fresh right-sized block (no cap slack), so pooling
/// would just waste a search for nothing. Shared with
/// `method_any_transform`'s splice (typed removed-array alloc).
#[inline]
pub(crate) unsafe fn arr_alloc_fresh(len: u64, cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * 8;
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
        // B1 — data pointer starts self-referential (inline slots).
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
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
        crate::layout::copy_elem_desc_bits(arr, p);
        if out_len > 0 {
            let src = data_ptr_at(arr, lo as usize);
            let dst = arr_data(p);
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
/// - `FLAG_ARR_ANY` (NaN-box u64 slots) → per-slot copy +
///   NaN-box-safe `rc_inc` (each new slot takes an independent ref,
///   the source keeps its own; same ledger as `arr_extend_any`).
///   An exotic receiver (`FLAG_ARR_EXOTIC_INDEX`) reads per index
///   through `arr_index_get` instead — [[Get]] semantics for
///   accessor indexes and holes (one predictable branch for plain
///   arrays).
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
        // RFC 20260810 刀 D — the bulk copy has no slots behind the
        // tail; loud reject until slice grows real sparse support.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.slice\0".as_ptr(),
        ) {
            return crate::alloc::__torajs_arr_alloc_any(0);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let (lo, hi) = clamp_range(len as i64, start, end);
        let out_len = (hi - lo) as u64;
        let flags = (*(arr as *const HeapHeader)).flags;
        if flags & FLAG_ARR_ANY != 0 {
            // Any-arrays never deque-shift — head is always 0.
            let total = ARR_CELL_SIZE + (out_len as usize) * ANY_SLOT_BYTES;
            let p = malloc(total) as *mut u8;
            *(p as *mut u32) = 1; // refcount
            *(p.add(4) as *mut u16) = TAG_ARR;
            *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
            *(p.add(ARR_LEN_OFF) as *mut u64) = out_len;
            *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = out_len as u32;
            *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
            *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
            *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
            let exotic = flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0;
            for i in 0..out_len as usize {
                let av = if exotic {
                    // §23.1.3.25 / §23.1.3.34 ride [[Get]] — an
                    // accessor index reads through its getter and a
                    // hole answers undefined (RFC 20260721 刀 5
                    // follow-up: the raw slot of an accessor index
                    // is dead undefined). Owned +1 — the fresh slot
                    // adopts it. The getter may mutate the receiver;
                    // out_len stays the §23.1.3.34 length snapshot.
                    crate::index_any::__torajs_arr_index_get(arr as *const c_void, lo + i as i64)
                } else {
                    let av = *(arr_data(arr).add((lo as usize + i) * ANY_SLOT_BYTES) as *const u64);
                    __torajs_rc_inc(av as *mut c_void);
                    av
                };
                *(arr_data(p).add(i * ANY_SLOT_BYTES) as *mut u64) = av;
            }
            return p;
        }
        let p = __torajs_arr_slice(arr, start, end);
        let kind_bits = flags & ARR_ELEM_KIND_MASK;
        *(p.add(6) as *mut u16) |= kind_bits;
        if kind_bits >> torajs_rc::ARR_ELEM_KIND_SHIFT == ARR_KIND_HEAP {
            for i in 0..out_len as usize {
                let cell = *(arr_data(p).add(i * 8) as *const u64);
                __torajs_rc_inc(cell as *mut c_void);
            }
        }
        p
    }
}

/// `arr.toReversed()` where the receiver arrived through the Any-elem
/// lane (RFC 20260721-array-proto-cluster 刀 13b) — §23.1.3.33 step 5
/// reads the SOURCE high→low (`result[i] = Get(O, len-1-i)`), so an
/// accessor getter's mid-iteration mutations (length shrink, element
/// writes) are observed by the LATER, lower-index reads. A forward
/// gather + raw swap is NOT equivalent under side effects — this
/// kernel walks the spec order directly. `len` is the §23.1.3.33
/// step 2 snapshot; a shrunk source answers later reads through the
/// prototype digit keys (`arr_index_get` OOB continuation).
///
/// Exotic receivers ride per-index [[Get]] (owned +1 per slot); a
/// clean `Array<Any>` keeps the raw reverse-copy + NaN-box-safe
/// rc_inc fast path.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer with `FLAG_ARR_ANY` set.
/// Returned pointer is a fresh owned (+1 rc) dense `Array<Any>`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_to_reversed(arr: *const u8) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — same loud reject as `arr_any_slice`.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.toReversed\0".as_ptr(),
        ) {
            return crate::alloc::__torajs_arr_alloc_any(0);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let total = ARR_CELL_SIZE + (len as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = len as u32;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0;
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
        let flags = (*(arr as *const HeapHeader)).flags;
        let exotic = flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0;
        for i in 0..len as usize {
            let av = if exotic {
                crate::index_any::__torajs_arr_index_get(
                    arr as *const c_void,
                    (len as usize - 1 - i) as i64,
                )
            } else {
                let av =
                    *(arr_data(arr).add((len as usize - 1 - i) * ANY_SLOT_BYTES) as *const u64);
                __torajs_rc_inc(av as *mut c_void);
                av
            };
            *(arr_data(p).add(i * ANY_SLOT_BYTES) as *mut u64) = av;
        }
        p
    }
}
