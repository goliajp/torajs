//! Typed-array ↔ `Array<Any>` bridge helpers.
//!
//! Crossings of the raw-slot / NaN-box boundary live here:
//!
//! - [`typed_slot_anyvalue_borrowed`] — RFC 20260707 chunk 621: a
//!   typed array shared into a static `Arr<Any>` slot (T-11
//!   container widen) keeps its raw-slot layout; the static any-arr
//!   readers rebox each slot per the elem kind recorded at the
//!   coercion boundary.
//! - [`typed_push_pair`] / [`typed_set_grow`] / [`typed_fill_pair`]
//!   — chunk 622 write side: kind-coerced raw writes for the static
//!   any-arr writers (mismatch is a catchable TypeError, never a
//!   silent NaN-box store into a raw slot).
//! - [`coerce_raw_scalar`] — the scalar half of the S3-set coercion
//!   table, shared with `index_any::__torajs_arr_index_set`.
//! - [`__torajs_arr_extend_typed_into_any`] — concat's bulk append
//!   of typed src slots into an `Array<Any>` dst.
//!
//! Split out of `any.rs` (chunk 621) when the reader arm pushed the
//! file over the 500-line limit.

use core::ffi::c_void;

use torajs_rc::{
    ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, ARR_KIND_UNSET, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::any::{ANY_HEAP, ANY_SLOT_BYTES, ANY_UNDEF, slot_anyvalue_ptr};
use crate::grow::grow_data_buffer;
use crate::layout::{ARR_LEN_OFF, arr_data};

/// Head-offset slot (matches `any.rs` / `index_any.rs`).
const ARR_HEAD_OFF: usize = 20;
/// Cap slot offset (matches `any.rs`).
const ARR_CAP_LOW32_OFF: usize = 16;

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box pack. Tag scheme:
    /// 0=Null, 1=Bool, 2=I64, 3=F64 (bits), 4=Heap, 5=Undef.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump (no-ops
    /// for non-cell bit patterns and NULL).
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — universal heap dropper (NaN-box-safe).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-throw catchable errors (record + return).
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Scalar half of the S3-set `(kind, anyv tag) → raw slot repr`
/// coercion table (`index_any::__torajs_arr_index_set` / the chunk
/// 622 typed writers). `None` = the value can't store without
/// changing the array's element kind — including every HEAP case,
/// which the 3-bit kind can't verify against the static elem type.
pub(crate) fn coerce_raw_scalar(kind: u16, tag: u64, value: u64) -> Option<u64> {
    match (kind, tag) {
        // I64 slot: int direct; integral double narrows.
        (ARR_KIND_I64, 2) => Some(value),
        (ARR_KIND_I64, 3) => {
            let d = f64::from_bits(value);
            if d.fract() != 0.0 || !d.is_finite() {
                return None;
            }
            Some(d as i64 as u64)
        }
        // F64 slot: double bits direct; int widens.
        (ARR_KIND_F64, 3) => Some(value),
        (ARR_KIND_F64, 2) => Some((value as i64 as f64).to_bits()),
        (ARR_KIND_BOOL, 1) => Some(value),
        _ => None,
    }
}

/// Release a transferred `tag == 4` rc (no-op for immediates).
pub(crate) unsafe fn drop_pair(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// `ARR_KIND_*` → NaN-box pair tag (`box_to_any`'s scheme), or None
/// for `ARR_KIND_UNSET` (unmarked block — no tag can be derived).
pub(crate) fn kind_to_any_tag(kind: u16) -> Option<u64> {
    match kind {
        ARR_KIND_I64 => Some(2),
        ARR_KIND_F64 => Some(3),
        ARR_KIND_BOOL => Some(1),
        ARR_KIND_HEAP => Some(4),
        _ => None,
    }
}

/// Chunk 624 — rebox a FRESH typed block's raw slots into NaN-box
/// AnyValues in place and flip it to `FLAG_ARR_ANY` (slot strides
/// match at 8 bytes; a HEAP slot's box IS its pointer, so element
/// ownership is untouched; a NULL heap slot holes to undefined).
/// Folds a nonzero deque head down to 0 (left-shift is safe in
/// place — the source index never trails the destination).
///
/// ONLY legal on a block with no typed alias — the typed view's raw
/// loads would misread the boxed slots. The caller guarantees
/// freshness (e.g. concat's `arr_any_slice` seed, rc=1, private).
/// UNSET kind raises a catchable TypeError (a missed coercion
/// boundary — loud, never a silent misread).
///
/// # Safety
/// `arr` is a valid non-Any `Tag::Arr` heap pointer, rc=1, unaliased.
pub(crate) unsafe fn transition_fresh_to_any(arr: *mut u8) {
    unsafe {
        let header = &mut *(arr as *mut HeapHeader);
        let kind = header.arr_elem_kind();
        let Some(tag) = kind_to_any_tag(kind) else {
            debug_assert!(
                false,
                "transition_fresh_to_any: UNSET elem kind — an Arr<T> → \
                 Arr<Any> coercion site missed __torajs_arr_mark_kind"
            );
            __torajs_throw_type_error(
                c"array of unknown element kind reached an any[] concat seed".as_ptr(),
            );
            return;
        };
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = arr_data(arr);
        for i in 0..len {
            let raw = *(data.add(((head + i) as usize) * ANY_SLOT_BYTES) as *const u64);
            let av = if tag == 4 {
                if raw == 0 {
                    __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0)
                } else {
                    raw // a heap cell's box encoding is its pointer
                }
            } else {
                __torajs_anyv_box_from_pair(tag as i64, raw as i64)
            };
            *(data.add((i as usize) * ANY_SLOT_BYTES) as *mut u64) = av;
        }
        *(arr.add(ARR_HEAD_OFF) as *mut u32) = 0;
        header.flags = (header.flags & !torajs_rc::ARR_ELEM_KIND_MASK) | torajs_rc::FLAG_ARR_ANY;
    }
}

/// Chunk 622 — `push_any`'s typed arm: kind-coerce the pair into a
/// raw slot and append via the typed `__torajs_arr_push`. A HEAP
/// slot takes the transferred reference directly (same admit as
/// `method_any`'s push table); a kind mismatch releases the
/// transferred rc and raises a catchable TypeError.
///
/// # Safety
/// `arr` is a valid non-Any `Tag::Arr` heap pointer; a `tag == 4`
/// `value` is 0 or a valid owned heap pointer.
pub(crate) unsafe fn typed_push_pair(arr: *mut u8, tag: u64, value: u64) -> *mut u8 {
    unsafe {
        let kind = (*(arr as *const HeapHeader)).arr_elem_kind();
        let raw = if kind == ARR_KIND_HEAP && tag == 4 {
            value // ownership transfers straight into the raw slot
        } else {
            match coerce_raw_scalar(kind, tag, value) {
                Some(r) => r,
                None => {
                    drop_pair(tag, value);
                    __torajs_throw_type_error(
                        c"push through an any[] view would change the typed array's element kind"
                            .as_ptr(),
                    );
                    return arr;
                }
            }
        };
        crate::grow::__torajs_arr_push(arr, raw as i64)
    }
}

/// Chunk 628 — `unshift_any`'s typed arm ([`typed_push_pair`]'s
/// prepend twin, the station chunk 622 missed): kind-coerce the pair
/// into a raw slot and prepend via the typed `__torajs_arr_unshift`.
/// A HEAP slot takes the transferred reference directly; a kind
/// mismatch releases the transferred rc and raises a catchable
/// TypeError.
///
/// # Safety
/// Same contract as [`typed_push_pair`].
pub(crate) unsafe fn typed_unshift_pair(arr: *mut u8, tag: u64, value: u64) -> *mut u8 {
    unsafe {
        let kind = (*(arr as *const HeapHeader)).arr_elem_kind();
        let raw = if kind == ARR_KIND_HEAP && tag == 4 {
            value // ownership transfers straight into the raw slot
        } else {
            match coerce_raw_scalar(kind, tag, value) {
                Some(r) => r,
                None => {
                    drop_pair(tag, value);
                    __torajs_throw_type_error(
                        c"unshift through an any[] view would change the typed array's element kind"
                            .as_ptr(),
                    );
                    return arr;
                }
            }
        };
        crate::transform::__torajs_arr_unshift(arr, raw as i64)
    }
}

/// Chunk 622 — `set_any_grow`'s typed arm. In-bounds writes delegate
/// to the kind-aware `arr_index_set` (same transfer ABI); `i == len`
/// is an append (`a[a.length] = v`) through [`typed_push_pair`]; a
/// past-the-end write kind-coerces the pair then grows-as-holes
/// (RFC 20260721-typed-grow-on-write — `typed_grow_store` zero-fills
/// the gap and HOLE-marks it; a cross-kind pair still throws, and a
/// beyond-dense-limit index raises the shared sparse RangeError).
///
/// # Safety
/// Same contract as [`typed_push_pair`].
pub(crate) unsafe fn typed_set_grow(arr: *mut u8, i: u64, tag: u64, value: u64) -> *mut u8 {
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if i < len {
            crate::index_any::__torajs_arr_index_set(arr as *mut c_void, i as i64, tag, value);
            return arr;
        }
        if i == len {
            return typed_push_pair(arr, tag, value);
        }
        if i >= crate::any::ARR_DENSE_LIMIT {
            drop_pair(tag, value);
            __torajs_throw_range_error(
                b"array index beyond the dense-storage limit (sparse arrays are not yet supported)\0"
                    .as_ptr(),
            );
            return arr;
        }
        let kind = (*(arr as *const HeapHeader)).arr_elem_kind();
        let raw = if kind == ARR_KIND_HEAP && tag == 4 {
            value // ownership transfers straight into the raw slot
        } else {
            match coerce_raw_scalar(kind, tag, value) {
                Some(r) => r,
                None => {
                    drop_pair(tag, value);
                    __torajs_throw_type_error(
                        c"index write through an any[] view would change the typed array's element kind"
                            .as_ptr(),
                    );
                    return arr;
                }
            }
        };
        crate::grow_store::typed_grow_store(arr, i, raw);
        arr
    }
}

/// Chunk 622 — `fill_any`'s typed arm: coerce the fill value once,
/// then raw-fill `[lo, hi)` honoring the deque head. The pair is a
/// BORROW (fill_any's contract — the Any path incs per replaced
/// slot), so a mismatch only throws; scalar slots carry no rc, so
/// the fill loop is a plain store. An `ARR_KIND_HEAP` receiver
/// accepts heap cells (and undefined → NULL hole) with the per-slot
/// drop-old + inc-new ledger (backfill chunk 2 — the 3-bit kind
/// still can't verify the static elem SUBtype, same admit as
/// `typed_push_pair`); other cross-kind pairs throw.
///
/// # Safety
/// `arr` is a valid non-Any `Tag::Arr` heap pointer with
/// `lo <= hi <= len`.
pub(crate) unsafe fn typed_fill_pair(arr: *mut u8, tag: u64, value: u64, lo: i64, hi: i64) {
    unsafe {
        let kind = (*(arr as *const HeapHeader)).arr_elem_kind();
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as u64;
        if kind == ARR_KIND_HEAP && (tag == ANY_HEAP || tag == ANY_UNDEF) {
            let raw = if tag == ANY_HEAP { value } else { 0 };
            for i in lo..hi {
                let slot =
                    arr_data(arr).add(((head + i as u64) as usize) * ANY_SLOT_BYTES) as *mut u64;
                __torajs_value_drop_heap(*slot as *mut c_void);
                *slot = raw;
                __torajs_rc_inc(raw as *mut c_void);
            }
            return;
        }
        let Some(raw) = coerce_raw_scalar(kind, tag, value) else {
            __torajs_throw_type_error(
                c"fill through an any[] view would change the typed array's element kind".as_ptr(),
            );
            return;
        };
        for i in lo..hi {
            *(arr_data(arr).add(((head + i as u64) as usize) * ANY_SLOT_BYTES) as *mut u64) = raw;
        }
    }
}

/// RFC 20260707 chunk 621 — borrowed AnyValue view of slot `i` on a
/// TYPED array (no FLAG_ARR_ANY) reached through a static `Arr<Any>`
/// view: the T-11 container widen shares the block without reboxing,
/// so it keeps its raw-slot layout and records its elem kind at the
/// coercion boundary (`emit_arr_mark_kind`). Rebox table mirrors
/// `index_any::__torajs_arr_index_get` — keep the two in sync.
/// Contract is BORROW: heap slots return without an rc bump (the
/// slot keeps its own reference), matching `any.rs`'s FLAG_ARR_ANY
/// read path — which is why this can't delegate to the +1-returning
/// `__torajs_arr_index_get`. Unlike Any-arrays, typed blocks can
/// have deque-shifted (`head_offset != 0`) data.
///
/// # Safety
/// `arr` is a valid non-Any `Tag::Arr` heap pointer with `i < len`.
pub(crate) unsafe fn typed_slot_anyvalue_borrowed(arr: *const u8, i: u64) -> u64 {
    unsafe {
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32) as u64;
        let raw = *(arr_data(arr).add(((head + i) as usize) * ANY_SLOT_BYTES) as *const u64);
        let header = &*(arr as *const HeapHeader);
        match header.arr_elem_kind() {
            ARR_KIND_I64 => __torajs_anyv_box_from_pair(2, raw as i64),
            ARR_KIND_F64 => __torajs_anyv_box_from_pair(3, raw as i64),
            ARR_KIND_BOOL => __torajs_anyv_box_from_pair(1, raw as i64),
            // A NULL heap slot is a hole — undefined per spec, never
            // a boxed null pointer.
            ARR_KIND_HEAP if raw == 0 => __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0),
            ARR_KIND_HEAP => __torajs_anyv_box_from_pair(ANY_HEAP as i64, raw as i64),
            kind => {
                debug_assert!(
                    kind == ARR_KIND_UNSET,
                    "arr_get_any: invalid elem kind {kind}"
                );
                debug_assert!(
                    false,
                    "arr_get_any: UNSET elem kind — an Arr<T> → Arr<Any> \
                     coercion site missed __torajs_arr_mark_kind"
                );
                __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0)
            }
        }
    }
}

/// `dst.concat(src, elem_tag)` append step — extends the Array<Any>
/// `dst` in place with `src`'s typed slots, each paired with
/// `elem_tag` and NaN-boxed. Same in-place + self-inc contract as
/// `__torajs_arr_extend_any`: grows via realloc when needed (caller
/// must adopt the returned pointer) and rc_incs each appended heap
/// cell itself, so the concat lowering never runs a raw inc walk
/// over NaN-box slots. `dst` must already be detached from the
/// receiver (the concat lowering seeds it with `arr_any_slice`).
///
/// The tag mirrors `box_to_any`'s scheme — 1=ANY_BOOL, 2=ANY_I64,
/// 3=ANY_F64, 4=ANY_HEAP. F64 src slots already hold raw IEEE bits
/// in u64 form (BitCastF64ToI64 form box_to_any uses); Bool src
/// slots hold 0/1 as i1 / u8 (ssa-lower `store i1` emits 1B; the
/// helper reads 1B to skip the upper 7 bytes of arr_alloc garbage).
///
/// # Safety
/// `dst` must be Array<Any> (FLAG_ARR_ANY, 8-byte AnyValue slots).
/// `src` must be a typed Array<T> with 8-byte slot stride (every
/// elem type — I64/F64/Bool/Heap — stores in 8B per slot, confirmed
/// via ssa-lower emit). `elem_tag` must match T's actual SSA type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_extend_typed_into_any(
    dst: *mut u8,
    src: *const u8,
    elem_tag: u64,
) -> *mut u8 {
    unsafe {
        // RFC 20260707 chunk 624 — the concat lowering statically
        // routes typed args here, but the arr_any_slice seed of a
        // typed-behind-Arr<Any> receiver is itself a kind-marked
        // typed COPY (fresh, rc=1) — rebox it in place before
        // splicing NaN-box values in.
        if (*(dst as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            transition_fresh_to_any(dst);
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
        // Box each typed src slot per the tag scheme. src is a typed
        // deque — fold its head offset in (a shifted src otherwise
        // reads slack slots).
        let src_head = *(src.add(ARR_HEAD_OFF) as *const u32) as usize;
        for i in 0..src_len {
            let slot_ptr = arr_data(src).add((src_head + i as usize) * 8);
            let raw = match elem_tag {
                1 => (slot_ptr as *const u8).read() as u64, // Bool — 1B store
                _ => (slot_ptr as *const u64).read(),       // I64 / F64 / Heap — 8B
            };
            // An INLINE substring view does not leave its split
            // block: the any slot takes an owned copy (rc 1 is the
            // slot's stake) instead of a pointer into a block that can
            // die first (rotation 468). Every other heap cell is
            // shared by one refcount — NaN-box-safe, no-op for
            // immediates (the slot takes an owning ref, the source
            // array keeps its own).
            let av = if elem_tag == ANY_HEAP
                && raw != 0
                && crate::substr_materialize::is_inline_view(raw as *const u8)
            {
                let owned = crate::substr_materialize::view_to_owned(raw as *const u8);
                __torajs_anyv_box_from_pair(elem_tag as i64, owned as i64)
            } else {
                let av = __torajs_anyv_box_from_pair(elem_tag as i64, raw as i64);
                __torajs_rc_inc(av as *mut c_void);
                av
            };
            *slot_anyvalue_ptr(dst, dst_len + i) = av;
        }
        *(dst.add(ARR_LEN_OFF) as *mut u64) = dst_len + src_len;
        dst
    }
}

/// RFC 20260708-typed-arr-oob-read chunk 2 — the lower-side OOB
/// exit for typed element lanes with no `undefined` representation
/// (I64 / Bool / nested-heap slots; Str answers the immortal
/// sentinel and F64 the undefined-NaN payload instead). Records a
/// catchable RangeError; the emit site's throw-check propagates it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_oob_throw() {
    unsafe {
        __torajs_throw_range_error(b"array index out of bounds\0".as_ptr());
    }
}
