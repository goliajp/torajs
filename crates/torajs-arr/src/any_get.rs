//! `Array<Any>` borrow-lane reads — the three `get_any_*` entries
//! ssa_lower emits for indexed loads (whole-box for HOF/sort/find
//! consumers, tag/value pair for the legacy FFI shape). Split from
//! `any.rs` (which keeps the append/write lanes) when the sparse-tail
//! gates (RFC 20260810-arr-sparse-grow) pushed it over the 500-line
//! file cap; same borrow contracts, moved verbatim.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, FLAG_ARR_EXOTIC_INDEX, HeapHeader};

use crate::any::{ANY_HEAP, ANY_UNDEF, slot_anyvalue_ptr};
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// Cross-tier — universal heap-value dropper (NaN-box-safe, skips
    /// immediates) for the accessor/hole product-caching slots.
    fn __torajs_value_drop_heap(p: *mut c_void);

    /// Cross-tier — torajs-anyvalue NaN-box pack/unpack.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// RFC 20260707 chunk 625 — borrowed whole-box read of slot `i`
/// for the inline SSA consumers (HOF loops / sort comparisons /
/// find family) whose `LoadDyn` raw read misread typed blocks
/// behind a static `Arr<Any>` view. Same borrow contract as
/// `LoadDyn` (the slot keeps its reference — heap boxes are NOT
/// +1'd), so emitted consumers need no rc changes. NULL / OOB
/// answer boxed `undefined` (ES §10.4.2.1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64 {
    unsafe {
        if arr.is_null() {
            return __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
        }
        let arr_u8 = arr as *const u8;
        let len = *(arr_u8.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            return __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
        }
        // Sparse tail (RFC 20260810) — no slot behind `[extent,
        // len)`; the borrow lanes answer undefined (they are already
        // proto-blind for explicit holes, whose cleared slot reads
        // undefined the same way).
        if (*(arr as *const HeapHeader)).flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
            && i >= crate::layout::arr_live_extent(arr_u8)
        {
            return __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY == 0 {
            return crate::any_typed_bridge::typed_slot_anyvalue_borrowed(arr_u8, i);
        }
        *slot_anyvalue_ptr(arr_u8 as *mut u8, i)
    }
}

/// `arr[i]` as an any value the CALLER OWNS — the borrowed read above
/// plus the stake, taken the right way per payload: a heap cell is
/// shared by one refcount, and an INLINE substring view (a split
/// product held as `any`, read slot by slot) is materialized into an
/// owned string instead, because its cell belongs to the split block
/// and would dangle the moment that block died (rotation 468; the
/// any-lane flatMap walk printed `["6","te!",…]`). Scalars and the
/// nullish immediates have nothing to own.
///
/// # Safety
///
/// `arr` is null or a live array cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_owned(arr: *const c_void, i: u64) -> u64 {
    let av = unsafe { __torajs_arr_get_any_boxed(arr, i) };
    // Cell-likeness first (rotation 546 form): a ShortStr reports tag
    // Heap and `unbox_value` would materialize an owned Str this probe
    // then abandons. A real cell's box IS the raw pointer, so the
    // inline-view test needs no unbox at all; every immediate (ShortStr
    // included) rides the NaN-box-safe rc_inc no-op below.
    if torajs_rc::ffi::nan_box_is_cell_like(av as *mut c_void) {
        let p = av as *const u8;
        if unsafe { crate::substr_materialize::is_inline_view(p) } {
            let owned = unsafe { crate::substr_materialize::view_to_owned(p) };
            return unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, owned as i64) };
        }
    }
    unsafe { torajs_rc::__torajs_rc_inc(av as *mut c_void) };
    av
}

/// OOB-safe read of slot `i`'s tag. NULL arr or `i >= len` returns
/// `ANY_UNDEF=5` per ES spec §10.4.2.1 (sparse array missing-index
/// semantics). A typed block behind the static `Arr<Any>` view
/// reboxes per elem kind (chunk 621).
///
/// Accessor index (RFC 20260713 chunk C): the getter runs HERE, once
/// per tag/value read pair, and its owned product is cached into the
/// element slot (drop-old + store) so the paired
/// [`__torajs_arr_get_any_value`] call reads the same product under
/// the existing borrow contract. The next tag read re-invokes the
/// getter, refreshing the cache.
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
        // Sparse tail — undefined, paired with `get_any_value`'s 0.
        // (Unlike the explicit-hole branch below there is no slot to
        // cache a prototype product into, so a prototype digit key
        // under a sparse tail is not consulted on this pair lane;
        // the `__torajs_arr_index_get` funnel handles it correctly.)
        if (*(arr as *const HeapHeader)).flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
            && i >= crate::layout::arr_live_extent(arr_u8)
        {
            return ANY_UNDEF;
        }
        if (*(arr as *const HeapHeader)).flags & FLAG_ARR_EXOTIC_INDEX != 0
            && (*(arr as *const HeapHeader)).flags & FLAG_ARR_ANY != 0
        {
            let pair = crate::define_accessor::__torajs_arr_index_accessor(arr, i);
            if !pair.is_null() {
                let product = crate::define_accessor::read_via_getter(pair, arr);
                let slot = slot_anyvalue_ptr(arr_u8 as *mut u8, i);
                __torajs_value_drop_heap((*slot) as *mut c_void);
                *slot = product;
                return __torajs_anyv_unbox_tag(product) as u64;
            }
            // 刀 5 G3 — a hole's [[Get]] continues to the prototype
            // digit keys; the owned answer caches into the element
            // slot (same pairing trick as the accessor product — the
            // shadow HOLE entry stays, so has/enumeration still see
            // the index as absent and the next tag read re-probes).
            if crate::define::__torajs_arr_index_flags(arr, i) & crate::define::F_HOLE != 0 {
                let product = crate::index_any::__torajs_arr_proto_index_get(arr, i as i64);
                let slot = slot_anyvalue_ptr(arr_u8 as *mut u8, i);
                __torajs_value_drop_heap((*slot) as *mut c_void);
                *slot = product;
                return __torajs_anyv_unbox_tag(product) as u64;
            }
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
        // Sparse tail — 0, pairing get_any_tag's ANY_UNDEF.
        if (*(arr as *const HeapHeader)).flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
            && i >= crate::layout::arr_live_extent(arr_u8)
        {
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
