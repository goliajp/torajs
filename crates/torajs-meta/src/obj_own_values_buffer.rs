//! §10.4.5.6 twin surfaces — `Object.values` / `Object.entries` over
//! the buffer family, split alongside `obj_own_keys_buffer.rs` (RFC
//! 20260823-typedarray-substrate own-property knife). A typed array
//! contributes its elements (re-derived length) then the expando-bag
//! tail; an ArrayBuffer contributes the bag alone — the keys face's
//! exact value twin, so all enumeration spellings agree.

use core::ffi::c_void;

use crate::obj_own_keys_layout::{
    ANY_HEAP_TAG, ARRAYBUFFER_PROPS_OFF, KIND_CHAIN_HEAP, TYPEDARRAY_PROPS_OFF,
};
use crate::obj_own_values::{dynobj_entries_append, dynobj_values_append};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, kind: u64);
    fn __torajs_i64_to_str(n: i64) -> *mut u8;
    fn __torajs_typedarray_length(v: u64) -> i64;
    /// §10.4.5.5-adjacent element read — a minted AnyValue (owned;
    /// numbers are imms, a BigInt element is a fresh +1 cell).
    fn __torajs_typedarray_index_get(recv: u64, index: f64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// The buffer-family cell's expando bag, if any (`tag` picks the
/// slot).
unsafe fn bag(cell: *const c_void, off: usize) -> *const c_void {
    unsafe { (cell.cast::<u8>().add(off) as *const u64).read() as *const c_void }
}

/// Typed-array `Object.values`: element values then the bag tail.
///
/// # Safety
/// `v` carries the live TypedArray cell `cell`.
pub(crate) unsafe fn typedarray_cell_values(v: u64, cell: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_typedarray_length(v) };
    let mut arr = unsafe { __torajs_arr_alloc_any(len.max(0) as u64) };
    for i in 0..len {
        // The minted element transfers its stake (a BigInt cell
        // arrives +1) — push consumes the pair, so no unbox_owned.
        let b = unsafe { __torajs_typedarray_index_get(v, i as f64) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value(b) };
        arr = unsafe { __torajs_arr_push_any(arr as *mut c_void, t as u64, val as u64) };
    }
    let props = unsafe { bag(cell, TYPEDARRAY_PROPS_OFF) };
    if props.is_null() {
        return arr as *mut c_void;
    }
    unsafe { dynobj_values_append(props, arr) as *mut c_void }
}

/// ArrayBuffer `Object.values`: the bag alone (promise-arm shape).
///
/// # Safety
/// `cell` is a live ArrayBuffer cell.
pub(crate) unsafe fn arraybuffer_cell_values(cell: *const c_void) -> *mut c_void {
    let arr = unsafe { __torajs_arr_alloc_any(0) };
    let props = unsafe { bag(cell, ARRAYBUFFER_PROPS_OFF) };
    if props.is_null() {
        return arr as *mut c_void;
    }
    unsafe { dynobj_values_append(props, arr) as *mut c_void }
}

/// Typed-array `Object.entries`: `[idx_str, elem]` pairs then the
/// bag tail, elem-kind stamped.
///
/// # Safety
/// `v` carries the live TypedArray cell `cell`.
pub(crate) unsafe fn typedarray_cell_entries(v: u64, cell: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_typedarray_length(v) };
    let mut outer = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let idx_str = unsafe { __torajs_i64_to_str(i) };
        let b = unsafe { __torajs_typedarray_index_get(v, i as f64) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value(b) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe {
            __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, idx_str as u64)
        };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, t as u64, val as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    let props = unsafe { bag(cell, TYPEDARRAY_PROPS_OFF) };
    if !props.is_null() {
        outer = unsafe { dynobj_entries_append(props, outer) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}

/// ArrayBuffer `Object.entries`: the bag pairs alone.
///
/// # Safety
/// `cell` is a live ArrayBuffer cell.
pub(crate) unsafe fn arraybuffer_cell_entries(cell: *const c_void) -> *mut c_void {
    let mut outer = unsafe { __torajs_arr_alloc(0) };
    let props = unsafe { bag(cell, ARRAYBUFFER_PROPS_OFF) };
    if !props.is_null() {
        outer = unsafe { dynobj_entries_append(props, outer) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}
