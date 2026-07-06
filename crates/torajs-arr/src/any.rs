//! `Array<Any>` substrate — tagged 8-byte slots (NaN-box AnyValue).
//! Port of `runtime_str.c` L414-582 (P4.1-d). Step 7e-A shrank the
//! slot stride 16 → 8 bytes by packing (tag, value) into a single
//! `AnyValue` u64; tag is inferred via `__torajs_anyv_unbox_tag` and
//! the legacy `(tag, value)` FFI pair is preserved so ssa_lower's IR
//! is unchanged.
//!
//! Layout: 24-byte header (rc/type_tag/flags + len + cap) +
//! `slot0: AnyValue u64 .. slotN`. `flags` carries `FLAG_ARR_ANY` so
//! `arr_free` skips the cap-matched (8-byte-stride) pool and
//! `arr_drop_any` is the correct walker; `type_tag` stays `TAG_ARR`
//! for the universal heap-walker; `head_offset` stays 0 (Any-arrays
//! never deque-shift).
//!
//! Public surface: `__torajs_arr_alloc_any` / `_alloc_any_filled` /
//! `_push_any` / `_extend_any` / `_get_any_tag` / `_get_any_value` /
//! `_set_any` / `_fill_any` / `_set_any_grow` / `_flat_any`
//! (`_extend_typed_into_any` lives in `any_typed_bridge`).

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, HeapHeader};

use crate::grow::grow_data_buffer;
use crate::layout::{
    ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, ARR_PROPS_OFF, TAG_ARR, arr_data,
};

/// Tag value for ANY_UNDEF — returned by OOB get to match JS spec.
pub(crate) const ANY_UNDEF: u64 = 5;

/// AnyValue tag for a heap-pointer-wrapped cell (Array<Any> elem
/// can wrap any heap value behind this tag — String, Array, Obj,
/// Closure, ...). Mirrors `torajs_rc::AnySlotTag` ANY_HEAP=4 (kept
/// inline here to avoid a crate-wide use just for one constant —
/// same shape as iter.rs / drop.rs).
pub(crate) const ANY_HEAP: u64 = 4;

/// 8 bytes — Array<Any> slot stride (Step 7e-A: NaN-box `AnyValue`
/// per slot; tag + value packed into one u64).
pub(crate) const ANY_SLOT_BYTES: usize = 8;

/// Cap slot offset (matches torajs-arr::alloc's `ARR_CAP_LOW32_OFF`).
const ARR_CAP_LOW32_OFF: usize = 16;
const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;

    /// Cross-tier — torajs-rc. Increments refcount; NULL pass-through
    /// (post-7d-A: also no-ops for non-cell NaN-box bit patterns).
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — universal heap-value dropper. Step 7d-A made it
    /// NaN-box-safe (skips immediates), so passing an `AnyValue`
    /// straight through is correct.
    fn __torajs_value_drop_heap(p: *mut c_void);

    /// Cross-tier — torajs-anyvalue NaN-box pack/unpack.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;

    /// Cross-tier — torajs-throw. Raises a catchable RangeError;
    /// the caller's SSA-level emit_throw_check propagates it.
    fn __torajs_throw_range_error(msg: *const u8);
}

#[inline]
pub(crate) unsafe fn slot_anyvalue_ptr(arr: *mut u8, i: u64) -> *mut u64 {
    unsafe { arr_data(arr).add((i as usize) * ANY_SLOT_BYTES) as *mut u64 }
}

#[inline]
unsafe fn write_header_any(p: *mut u8, len: u64, cap: u32) {
    unsafe {
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0; // Any-arrays never deque-shift
        // Round 4 chunk 5a — inline props_dynobj slot initialized to NULL.
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        // B1 — data pointer starts self-referential (inline slots).
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
    }
}

/// `__torajs_arr_alloc_any(cap)` — fresh empty Array<Any>.
/// Bypasses the regular Array<T> pool (different slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any(cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, 0, cap as u32);
        p
    }
}

/// `__torajs_arr_alloc_any_filled(n)` — `new Array(n)` per ES spec
/// §23.1.2.1. len=cap=n, all slots boxed `ANY_UNDEF` so `arr[i]`
/// decodes as `undefined` per ES §10.4.2.1 (sparse missing-index
/// semantics densely emulated; true sparse hole would need an
/// elem-kind-tag substrate, L3b). Mirrors the hole-fill pattern in
/// `__torajs_arr_set_at_any`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8 {
    // §23.1.2.1 step 4.b — a length outside [0, 2^32-1] is a
    // RangeError (test262-bug-corpus RC-4 F5: `new Array(-1)` arrives
    // as 0xFFFF..FF u64 and SIGSEGVd through the overflowed malloc).
    // Message matches bun/JSC for uncaught-print byte parity. The
    // throw helper RETURNS; ssa_lower's emit_throw_check right after
    // the call diverts before the NULL is touched.
    const MAX_ARRAY_LEN: u64 = u32::MAX as u64;
    if n > MAX_ARRAY_LEN {
        unsafe {
            __torajs_throw_range_error(
                b"Array length must be a positive integer of safe magnitude.\0".as_ptr(),
            );
        }
        return core::ptr::null_mut();
    }
    unsafe {
        let total = ARR_CELL_SIZE + (n as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, n, n as u32);
        if n > 0 {
            let undef = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
            for k in 0..n {
                *slot_anyvalue_ptr(p, k) = undef;
            }
        }
        p
    }
}

/// Append a tagged slot. Grows 2× on `len == cap` (matches C
/// arr_push's growth strategy). Returns the (possibly-realloc'd) array
/// pointer; caller stores it back into the binding slot, mirroring the
/// `arr_push` contract.
///
/// # Safety
/// `arr` must be a valid Array<Any> heap pointer (FLAG_ARR_ANY set,
/// 8-byte AnyValue slot stride). For ANY_HEAP slots the caller MUST
/// have pre-rc-incremented the heap value; push takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8 {
    let arr = arr as *mut u8;
    unsafe {
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Chunk 622 — typed block behind the static Arr<Any>
            // view: kind-coerce and append raw (mismatch TypeError).
            return crate::any_typed_bridge::typed_push_pair(arr, tag, value);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let cap = *(arr.add(ARR_CAP_LOW32_OFF) as *const u32);
        if (len as u32) == cap {
            let new_cap: u32 = if cap == 0 { 4 } else { cap * 2 };
            grow_data_buffer(arr, new_cap as u64);
        }
        let av = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        *slot_anyvalue_ptr(arr, len) = av;
        *(arr.add(ARR_LEN_OFF) as *mut u64) = len + 1;
        arr
    }
}

/// Extend `dst` with `src`'s tagged slots. Both are Array<Any>
/// (8-byte AnyValue slots). Each appended cell-tagged slot gets its
/// refcount bumped so dst shares ownership; src retains its own.
/// Reallocs dst when cap is insufficient (2× growth).
///
/// # Safety
/// Both `dst` and `src` must be valid Array<Any> heap pointers.
/// Caller MUST capture the return value (dst may have moved).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_extend_any(dst: *mut u8, src: *const u8) -> *mut u8 {
    unsafe {
        let dst_len = *(dst.add(ARR_LEN_OFF) as *const u64);
        let src_len = *(src.add(ARR_LEN_OFF) as *const u64);
        if src_len == 0 {
            return dst;
        }
        let cap = *(dst.add(ARR_CAP_LOW32_OFF) as *const u32);
        let needed = dst_len + src_len;
        if needed > cap as u64 {
            let mut new_cap: u32 = if cap == 0 { 4 } else { cap };
            while (new_cap as u64) < needed {
                new_cap *= 2;
            }
            grow_data_buffer(dst, new_cap as u64);
        }
        for i in 0..src_len {
            // src is technically *const u8; cast via *mut for the
            // shared slot accessor (read-only is fine).
            let src_mut = src as *mut u8;
            let av = *slot_anyvalue_ptr(src_mut, i);
            // rc_inc is NaN-box-safe — no-op for non-cell immediates,
            // bumps the wrapped heap pointer's refcount for cells.
            __torajs_rc_inc(av as *mut c_void);
            *slot_anyvalue_ptr(dst, dst_len + i) = av;
        }
        *(dst.add(ARR_LEN_OFF) as *mut u64) = dst_len + src_len;
        dst
    }
}

/// OOB-safe read of slot `i`'s tag. NULL arr or `i >= len` returns
/// `ANY_UNDEF=5` per ES spec §10.4.2.1 (sparse array missing-index
/// semantics). A typed block behind the static `Arr<Any>` view
/// reboxes per elem kind (chunk 621).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64 {
    if arr.is_null() {
        return ANY_UNDEF;
    }
    unsafe {
        let arr_u8 = arr as *const u8;
        let len = *(arr_u8.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            return ANY_UNDEF;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            return __torajs_anyv_unbox_tag(crate::any_typed_bridge::typed_slot_anyvalue_borrowed(
                arr_u8, i,
            )) as u64;
        }
        let av = *slot_anyvalue_ptr(arr_u8 as *mut u8, i);
        __torajs_anyv_unbox_tag(av) as u64
    }
}

/// OOB-safe read of slot `i`'s value. NULL arr or `i >= len` returns
/// 0 (paired with ANY_UNDEF tag from `get_any_tag` to spec-match
/// sparse-array reads). A typed block behind the static `Arr<Any>`
/// view reboxes per elem kind (chunk 621); the heap-kind arm stays
/// a borrow, same as the FLAG_ARR_ANY path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64 {
    if arr.is_null() {
        return 0;
    }
    unsafe {
        let arr_u8 = arr as *const u8;
        let len = *(arr_u8.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            return 0;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            return __torajs_anyv_unbox_value(crate::any_typed_bridge::typed_slot_anyvalue_borrowed(
                arr_u8, i,
            )) as u64;
        }
        let av = *slot_anyvalue_ptr(arr_u8 as *mut u8, i);
        __torajs_anyv_unbox_value(av) as u64
    }
}

/// Indexed write — `arr[i] = (tag, value)`. NULL arr is a no-op. If
/// the slot previously held a heap-cell AnyValue, drop it first to
/// keep refcount accounting balanced (`value_drop_heap` is
/// NaN-box-safe — primitives no-op).
///
/// bug-327 C3 — OOB `i` raises a catchable RangeError instead of the
/// pre-fix unchecked write (which first treated adjacent heap bytes
/// as a droppable AnyValue — arbitrary deref+free — then wrote past
/// the block: silent corruption for small `i`, SIGSEGV past the
/// page). This entry stays on receivers with no write-back slot
/// (`getArr()[i] = v`); growable receivers route through
/// [`__torajs_arr_set_any_grow`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64) {
    if arr.is_null() {
        return;
    }
    let arr = arr as *mut u8;
    unsafe {
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Chunk 622 — typed block behind the static Arr<Any>
            // view: in-bounds kind-coerced write / `i == len`
            // append / past-the-end RangeError. The returned cell is
            // always `arr` itself (B1 fixed-cell: grow swaps the
            // data buffer, never the cell), so the missing
            // write-back slot on this entry is safe to ignore.
            crate::any_typed_bridge::typed_set_grow(arr, i, tag, value);
            return;
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            __torajs_throw_range_error(
                b"out-of-bounds index write through a temporary array receiver is not yet supported\0".as_ptr(),
            );
            return;
        }
        let old_av = *slot_anyvalue_ptr(arr, i);
        __torajs_value_drop_heap(old_av as *mut c_void);
        *slot_anyvalue_ptr(arr, i) = __torajs_anyv_box_from_pair(tag as i64, value as i64);
    }
}

/// `arr.fill((tag, value), start, end)` for Array<Any> — write the
/// NaN-boxed AnyValue into each slot in `[start, end)`. Indices are
/// clamped to `[0, len]`. Each pre-existing slot is dropped first
/// (value_drop_heap is NaN-box-safe, no-ops on primitives) so a
/// fill over heap-tagged ANY_HEAP slots doesn't leak; the fill
/// value is rc_inc'd per replaced slot when it's an ANY_HEAP cell
/// so each slot owns a balanced ref. Mirrors the typed
/// `__torajs_arr_fill` contract (returns the same pointer; never
/// reallocs).
///
/// # Safety
/// `arr` must be a valid Array<Any> heap block (FLAG_ARR_ANY,
/// 8-byte AnyValue slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_fill_any(
    arr: *mut c_void,
    tag: u64,
    value: u64,
    start: i64,
    end: i64,
) -> *mut u8 {
    let arr = arr as *mut u8;
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64) as i64;
        let lo = if start < 0 {
            0
        } else if start > len {
            len
        } else {
            start
        };
        let hi = if end < 0 {
            0
        } else if end > len {
            len
        } else {
            end
        };
        if hi <= lo {
            return arr;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Chunk 622 — typed block behind the static Arr<Any>
            // view: raw-fill per the elem kind (mismatch TypeError).
            crate::any_typed_bridge::typed_fill_pair(arr, tag, value, lo, hi);
            return arr;
        }
        let av = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        for i in lo..hi {
            let slot = slot_anyvalue_ptr(arr, i as u64);
            let old_av = *slot;
            __torajs_value_drop_heap(old_av as *mut c_void);
            *slot = av;
            // rc_inc is NaN-box-safe — no-op for primitives, bumps
            // the wrapped heap pointer's refcount for ANY_HEAP cells.
            __torajs_rc_inc(av as *mut c_void);
        }
        arr
    }
}

/// ES-spec dense limit for the growable indexed-write path. Writing
/// at an index past this raises RangeError — torajs arrays are dense
/// (no dictionary-mode fallback yet), so `x[4294967295] = 1`-style
/// sparse writes would otherwise demand a 32GB realloc. Matches V8's
/// fast-elements order of magnitude (32M slots).
const ARR_DENSE_LIMIT: u64 = 32 * 1024 * 1024;

/// bug-327 C3 — bounds-honoring indexed write for receivers with a
/// write-back slot (`a[i] = v` on an Ident binding). `i < len`
/// behaves like [`__torajs_arr_set_any`]; `i >= len` grows per ES
/// spec (§10.4.2.1 OrdinarySet on an array index): reserve `i+1`
/// slots (2× amortized), fill the `len..i` gap with undefined, set
/// `len = i+1`. Returns the (possibly-realloc'd) array pointer; the
/// caller MUST store it back, mirroring the `arr_push_any` contract.
///
/// # Safety
/// `arr` must be a valid Array<Any> heap pointer (FLAG_ARR_ANY,
/// 8-byte AnyValue slots). For ANY_HEAP values the caller has
/// pre-rc-incremented; the slot takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_any_grow(
    arr: *mut c_void,
    i: u64,
    tag: u64,
    value: u64,
) -> *mut u8 {
    let arr = arr as *mut u8;
    unsafe {
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Chunk 622 — typed block behind the static Arr<Any>
            // view: in-bounds kind-coerced write / `i == len`
            // append / past-the-end RangeError.
            return crate::any_typed_bridge::typed_set_grow(arr, i, tag, value);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if i < len {
            let old_av = *slot_anyvalue_ptr(arr, i);
            __torajs_value_drop_heap(old_av as *mut c_void);
            *slot_anyvalue_ptr(arr, i) = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            return arr;
        }
        if i >= ARR_DENSE_LIMIT {
            __torajs_throw_range_error(
                b"array index beyond the dense-storage limit (sparse arrays are not yet supported)\0".as_ptr(),
            );
            return arr;
        }
        let cap = *(arr.add(ARR_CAP_LOW32_OFF) as *const u32) as u64;
        if i >= cap {
            let mut new_cap: u64 = if cap == 0 { 4 } else { cap * 2 };
            if new_cap < i + 1 {
                new_cap = i + 1;
            }
            grow_data_buffer(arr, new_cap);
        }
        let undef = __torajs_anyv_box_from_pair(5, 0); // ANY_UNDEF
        for k in len..i {
            *slot_anyvalue_ptr(arr, k) = undef;
        }
        *slot_anyvalue_ptr(arr, i) = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        *(arr.add(ARR_LEN_OFF) as *mut u64) = i + 1;
        arr
    }
}

/// `arr.flat()` depth-1 for Array<Any> outer. Each outer slot is
/// decoded — when it wraps an inner Array<Any> heap (tag = ANY_HEAP,
/// inner type_tag = TAG_ARR, inner FLAG_ARR_ANY set) the inner's
/// slots are appended via `__torajs_arr_extend_any` (which handles
/// per-cell rc_inc for shared ownership). Other slots (scalars or
/// non-Arr heap values) carry through as a single push so non-
/// arrayish elements survive flatten per ES §23.1.3.13.
///
/// v0 supports depth=1 only — matches the typed `__torajs_arr_flat`
/// contract. depth=N is unrolled at the ssa-lower layer (mirror of
/// the existing typed-flat dispatch in `ssa_lower_str.rs`).
///
/// # Safety
/// `outer` must be a valid Array<Any> heap pointer (FLAG_ARR_ANY,
/// 8-byte AnyValue slots). Inner Array<Any> pointers carried in
/// ANY_HEAP slots stay alive for the duration of the call — caller
/// holds the only reference and we walk slots before dst is filled,
/// so no drop races occur even if dst grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_flat_any(outer: *const u8) -> *mut u8 {
    let outer_len = unsafe { *(outer.add(ARR_LEN_OFF) as *const u64) };
    // Start with cap == outer_len; arr_push_any / arr_extend_any
    // grow on demand so over-estimating isn't required.
    let mut dst = unsafe { __torajs_arr_alloc_any(outer_len) };
    let outer_mut = outer as *mut u8;
    for i in 0..outer_len {
        let av = unsafe { *slot_anyvalue_ptr(outer_mut, i) };
        let tag = unsafe { __torajs_anyv_unbox_tag(av) } as u64;
        let value = unsafe { __torajs_anyv_unbox_value(av) };
        if tag == ANY_HEAP {
            let inner = value as *const u8;
            if !inner.is_null() {
                let inner_type_tag = unsafe { *(inner.add(4) as *const u16) };
                let inner_flags = unsafe { *(inner.add(6) as *const u16) };
                if inner_type_tag == TAG_ARR && (inner_flags & FLAG_ARR_ANY) != 0 {
                    dst = unsafe { __torajs_arr_extend_any(dst, inner) };
                    continue;
                }
            }
        }
        // Non-array slot — preserve as one element. push_any takes
        // ownership of ANY_HEAP cells, so bump the refcount before
        // handing it over (mirrors arr_extend_any's per-slot
        // rc_inc loop).
        unsafe { __torajs_rc_inc(av as *mut c_void) };
        dst = unsafe { __torajs_arr_push_any(dst as *mut c_void, tag, value as u64) };
    }
    dst
}
