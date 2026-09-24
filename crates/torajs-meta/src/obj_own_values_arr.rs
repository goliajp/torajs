//! `Object.values` / `Object.entries` over an Arr cell — split from
//! [`crate::obj_own_values`] (file-size limit): the kind-aware element
//! walk, then the props-dynobj expando tail through the DynObj walk's
//! appenders.

use core::ffi::c_void;

use crate::obj_own_keys::{ANY_HEAP_TAG, ARR_LEN_OFF, ARR_PROPS_OFF, KIND_CHAIN_HEAP};
use crate::obj_own_values::{dynobj_entries_append, dynobj_values_append};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    /// torajs-arr kind-aware borrowed whole-box slot read.
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_i64_to_str(n: i64) -> *mut u8;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
}

/// Arr-cell values: kind-aware element walk (borrowed box read →
/// owned unbox per slot), then the props-dynobj expando tail.
pub(crate) unsafe fn arr_cell_values(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut arr = unsafe { __torajs_arr_alloc_any(len) };
    for i in 0..len {
        let b = unsafe { __torajs_arr_get_any_boxed(cell, i) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(b) };
        arr = unsafe { __torajs_arr_push_any(arr as *mut c_void, t as u64, val as u64) };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return arr as *mut c_void;
    }
    unsafe { dynobj_values_append(props, arr) as *mut c_void }
}

/// Arr-cell entries: `[[idx_str, elem], ...]` pairs plus the expando
/// tail, elem-kind stamped.
pub(crate) unsafe fn arr_cell_entries(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut outer = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let idx_str = unsafe { __torajs_i64_to_str(i as i64) };
        let b = unsafe { __torajs_arr_get_any_boxed(cell, i) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(b) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe {
            __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, idx_str as u64)
        };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, t as u64, val as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if !props.is_null() {
        outer = unsafe { dynobj_entries_append(props, outer) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}
