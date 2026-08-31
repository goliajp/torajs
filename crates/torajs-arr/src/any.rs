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
//! Public surface: `_push_any` / `_extend_any` / `_get_any_tag` /
//! `_get_any_value` / `_set_any` / `_fill_any` / `_set_any_grow` /
//! `_flat_any` (`_alloc_any` / `_alloc_any_filled` live in `alloc`,
//! `_extend_typed_into_any` in `any_typed_bridge`).

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, FLAG_ARR_EXOTIC_INDEX, HeapHeader};

use crate::grow::grow_data_buffer;
use crate::layout::{ARR_LEN_OFF, TAG_ARR, arr_data};

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

unsafe extern "C" {
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

    /// Cross-tier — torajs-throw catchable TypeError (chunk 624,
    /// unknown-elem-kind concat rejections).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

// Borrow-lane reads moved to `any_get` (500-line file cap); the
// `crate::any::` face stays valid for the in-crate callers.
pub use crate::any_get::{
    __torajs_arr_get_any_boxed, __torajs_arr_get_any_tag, __torajs_arr_get_any_value,
};

#[inline]
pub(crate) unsafe fn slot_anyvalue_ptr(arr: *mut u8, i: u64) -> *mut u64 {
    unsafe { arr_data(arr).add((i as usize) * ANY_SLOT_BYTES) as *mut u64 }
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
        // RFC 20260810 刀 D — the append slot sits past the
        // materialized extent; loud reject (the transferred value is
        // released to keep the pair ledger balanced).
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.push\0".as_ptr(),
        ) {
            if tag == ANY_HEAP {
                __torajs_value_drop_heap(value as *mut c_void);
            }
            return arr;
        }
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

/// Append one BORROWED NaN-box AnyValue, storing the box bits
/// directly — never unboxing. The pair-shaped sibling above forces
/// callers holding a box through unbox/rebox, and `unbox_value`
/// materializes a ShortStr into a fresh heap Str the rebox then
/// abandons (rotation 546: the any-concat append leaked one Str per
/// round exactly this way; same rule as `arr_new_from_any`'s element
/// path). The slot takes its own +1 via the NaN-box-safe rc_inc.
/// Returns the (possibly-realloc'd) array pointer; caller stores it
/// back.
///
/// # Safety
/// `arr` must be a valid Array heap pointer; `av` carries a valid
/// AnyValue bit pattern, borrowed from the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_any_boxed(arr: *mut u8, av: u64) -> *mut u8 {
    unsafe {
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.push\0".as_ptr(),
        ) {
            return arr;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            // Typed block behind a static Arr<Any> view: raw slots,
            // so the pair spelling is the storage format anyway — a
            // materialized ShortStr is adopted by the slot, not
            // abandoned. typed_push_pair ADOPTS, this entry BORROWS:
            // a heap cell needs its stake handed over (+1); a
            // materialized ShortStr already carries a fresh one.
            let tag = __torajs_anyv_unbox_tag(av) as u64;
            let value = __torajs_anyv_unbox_value(av) as u64;
            if tag == ANY_HEAP {
                __torajs_rc_inc(value as *mut c_void);
            }
            return crate::any_typed_bridge::typed_push_pair(arr, tag, value);
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let cap = *(arr.add(ARR_CAP_LOW32_OFF) as *const u32);
        if (len as u32) == cap {
            let new_cap: u32 = if cap == 0 { 4 } else { cap * 2 };
            grow_data_buffer(arr, new_cap as u64);
        }
        __torajs_rc_inc(av as *mut c_void);
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
        // RFC 20260810 刀 D — both sides walk raw slots: dst appends
        // at `dst_len` (no slot behind a sparse dst), src reads
        // `[0, src_len)`. Loud reject on either.
        let msg = b"sparse array tail is not yet supported in an array concat/spread\0".as_ptr();
        if crate::sparse_gate::sparse_tail_rejects(dst as *const c_void, msg)
            || crate::sparse_gate::sparse_tail_rejects(src as *const c_void, msg)
        {
            return dst;
        }
        // RFC 20260707 chunk 624 — typed blocks on either side of
        // the concat splice. The dst seed comes from arr_any_slice,
        // which hands a typed source back as a kind-marked typed
        // COPY (fresh, rc=1, unaliased) — rebox it in place before
        // splicing NaN-box values in. A typed src bulk-boxes through
        // the extend_typed bridge per its recorded kind; UNSET is a
        // missed coercion boundary (loud TypeError inside).
        if (*(dst as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            crate::any_typed_bridge::transition_fresh_to_any(dst);
        }
        if (*(src as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            let kind = (*(src as *const HeapHeader)).arr_elem_kind();
            let Some(tag) = crate::any_typed_bridge::kind_to_any_tag(kind) else {
                __torajs_throw_type_error(
                    c"array of unknown element kind reached an any[] concat".as_ptr(),
                );
                return dst;
            };
            return crate::any_typed_bridge::__torajs_arr_extend_typed_into_any(dst, src, tag);
        }
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

/// Indexed write — `arr[i] = (tag, value)`. NULL arr is a no-op. If
/// the slot previously held a heap-cell AnyValue, drop it first to
/// keep refcount accounting balanced (`value_drop_heap` is
/// NaN-box-safe — primitives no-op).
///
/// Indexed write on a receiver with no write-back slot
/// (`getArr()[i] = v`, `Array.prototype[0] = v`).
///
/// Identical to [`__torajs_arr_set_any`]'s growable sibling — the
/// missing write-back slot is not a reason to refuse an out-of-bounds
/// write, because B1 made the cell fixed: a grow swaps the data
/// buffer behind it and hands back the same pointer. (It WAS a
/// reason, before B1, which is why this entry used to raise
/// "out-of-bounds index write through a temporary array receiver is
/// not yet supported" — the typed arm below already stopped doing
/// that. `Array.prototype[0] = false` is ordinary ES §10.4.2.1
/// OrdinarySet, and it is what test262 uses to set up an inherited
/// index property.)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64) {
    if arr.is_null() {
        return;
    }
    // The returned cell is always `arr` itself, so dropping it is safe.
    let _ = unsafe { __torajs_arr_set_any_grow(arr, i, tag, value) };
}

/// ES-spec dense limit for the growable indexed-write path. Writing
/// at an index past this raises RangeError — torajs arrays are dense
/// (no dictionary-mode fallback yet), so `x[4294967295] = 1`-style
/// sparse writes would otherwise demand a 32GB realloc. Matches V8's
/// fast-elements order of magnitude (32M slots).
pub(crate) const ARR_DENSE_LIMIT: u64 = 32 * 1024 * 1024;

/// bug-327 C3 — bounds-honoring indexed write for receivers with a
/// write-back slot (`a[i] = v` on an Ident binding). `i < len`
/// behaves like [`__torajs_arr_set_any`]; `i >= len` grows per ES
/// spec (§10.4.2.1 OrdinarySet on an array index): reserve `i+1`
/// slots (2× amortized), fill the `len..i` gap as HOLES (刀 5 G3 —
/// undefined-reading slots that are not own properties), set
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
        // Sparse tail (RFC 20260810) — an index with no slot behind
        // it, in-tail OR past the end (the plain grow lane below
        // widens `cap` without materializing `[extent, len)`, which
        // would corrupt the sparse invariant — every no-slot write
        // funnels here instead). Below the dense limit the write
        // materializes the extension exactly (cap == extent is the
        // sparse invariant, so growth is exact, not 2x-amortized;
        // the gap becomes explicit holes); past the limit it is the
        // same loud RangeError as the past-the-end lane.
        if (*(arr as *const HeapHeader)).flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0 {
            let extent = crate::layout::arr_live_extent(arr);
            if i >= extent {
                if i >= ARR_DENSE_LIMIT {
                    __torajs_throw_range_error(
                        b"array index beyond the dense-storage limit (sparse arrays are not yet supported)\0".as_ptr(),
                    );
                    return arr;
                }
                grow_data_buffer(arr, i + 1);
                let undef = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
                for k in extent..i {
                    *slot_anyvalue_ptr(arr, k) = undef;
                }
                *slot_anyvalue_ptr(arr, i) = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                if i > extent {
                    crate::define_hole::mark_hole_range(arr as *mut c_void, extent, i);
                }
                // cap == extent again; the crossover back to a fully
                // dense cell clears the flag (an append past `len`
                // also extends it, §10.4.2.1 OrdinarySet).
                if i + 1 >= len {
                    if i + 1 > len {
                        *(arr.add(ARR_LEN_OFF) as *mut u64) = i + 1;
                    }
                    (*(arr as *mut HeapHeader)).flags &= !torajs_rc::FLAG_ARR_SPARSE_TAIL;
                }
                return arr;
            }
        }
        if i < len {
            // Exotic slow path (pre-store) — an accessor index writes
            // through its setter, never element storage (chunk C).
            if (*(arr as *const HeapHeader)).flags & FLAG_ARR_EXOTIC_INDEX != 0 {
                let pair =
                    crate::define_accessor::__torajs_arr_index_accessor(arr as *const c_void, i);
                if !pair.is_null() {
                    crate::define_accessor::write_via_setter(
                        pair,
                        arr as *const c_void,
                        tag,
                        value,
                    );
                    return arr;
                }
            }
            let old_av = *slot_anyvalue_ptr(arr, i);
            __torajs_value_drop_heap(old_av as *mut c_void);
            *slot_anyvalue_ptr(arr, i) = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            // A write into a deleted (hole) index re-creates it as a
            // default data property (chunk C).
            if (*(arr as *const HeapHeader)).flags & FLAG_ARR_EXOTIC_INDEX != 0 {
                crate::define_hole::revive_index_if_hole(arr as *mut c_void, i);
            }
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
        // 刀 5 G3 — a past-the-end write's GAP indices `[len, i)` are
        // holes, not own undefineds (§10.4.2.1). The plain append
        // (`i == len`, the hot array-build shape) has no gap and
        // never pays the call.
        if i > len {
            crate::define_hole::mark_hole_range(arr as *mut c_void, len, i);
        }
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
    // RFC 20260810 刀 D — the outer walk reads raw slots; loud
    // reject (a sparse INNER array is caught by `extend_any`).
    if unsafe {
        crate::sparse_gate::sparse_tail_rejects(
            outer as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.flat\0".as_ptr(),
        )
    } {
        return unsafe { crate::alloc::__torajs_arr_alloc_any(0) };
    }
    let outer_len = unsafe { *(outer.add(ARR_LEN_OFF) as *const u64) };
    // Start with cap == outer_len; arr_push_any / arr_extend_any
    // grow on demand so over-estimating isn't required.
    let mut dst = unsafe { crate::alloc::__torajs_arr_alloc_any(outer_len) };
    let outer_mut = outer as *mut u8;
    for i in 0..outer_len {
        let av = unsafe { *slot_anyvalue_ptr(outer_mut, i) };
        let tag = unsafe { __torajs_anyv_unbox_tag(av) } as u64;
        let value = unsafe { __torajs_anyv_unbox_value(av) };
        if tag == ANY_HEAP {
            let inner = value as *const u8;
            if !inner.is_null() {
                let inner_type_tag = unsafe { *(inner.add(4) as *const u16) };
                // Chunk 624 — typed inner arrays flatten too:
                // extend_any's src dispatch bulk-boxes them per
                // their recorded elem kind (pre-fix they carried
                // through as a single scalar-looking element).
                if inner_type_tag == TAG_ARR {
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
