//! §10.4.5.6 / §25.1 — own keys of the buffer family, split from
//! `obj_own_keys.rs` (file-size cap; RFC 20260823-typedarray-substrate
//! own-property knife). A typed array owns its in-bounds canonical
//! indices plus expando-bag string keys; an ArrayBuffer owns only the
//! bag. NO trailing "length" on either — those are prototype
//! accessors.

use core::ffi::c_void;

use crate::obj_own_keys::dynobj_keys_append;
use crate::obj_own_keys_layout::{ARRAYBUFFER_PROPS_OFF, TYPEDARRAY_PROPS_OFF};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    /// torajs-buffer — §23.2.4.1 the view's CURRENT element count
    /// (re-derived: tracking views move with their buffer).
    fn __torajs_typedarray_length(v: u64) -> i64;
}

/// Typed-array own keys: index list (re-derived length) then the
/// expando bag in insertion order.
///
/// # Safety
/// `v` carries the live TypedArray cell `cell`.
pub(crate) unsafe fn typedarray_cell_keys(
    v: u64,
    cell: *const c_void,
    include_nonenum: i64,
) -> *mut c_void {
    let len = unsafe { __torajs_typedarray_length(v) };
    let out = unsafe { crate::own_names::__torajs_arr_keys_only(len) } as *mut u8;
    let props = unsafe { (cell.cast::<u8>().add(TYPEDARRAY_PROPS_OFF) as *const u64).read() }
        as *const c_void;
    if props.is_null() {
        out as *mut c_void
    } else {
        unsafe { dynobj_keys_append(props, include_nonenum, out, false, false) as *mut c_void }
    }
}

/// ArrayBuffer own keys: the expando bag alone (promise-arm shape).
///
/// # Safety
/// `cell` is a live ArrayBuffer cell.
pub(crate) unsafe fn arraybuffer_cell_keys(
    cell: *const c_void,
    include_nonenum: i64,
) -> *mut c_void {
    let props = unsafe { (cell.cast::<u8>().add(ARRAYBUFFER_PROPS_OFF) as *const u64).read() }
        as *const c_void;
    let out = unsafe { __torajs_arr_alloc(0) };
    if props.is_null() {
        out as *mut c_void
    } else {
        unsafe { dynobj_keys_append(props, include_nonenum, out, false, false) as *mut c_void }
    }
}
