//! W-J Phase C — struct-cell enumeration arms for `Object.keys`
//! (Phase C1), with `values` / `entries` following in C2 / C3.
//!
//! tora lowers `Object.keys(o)` to a compile-time field-name literal
//! when `o`'s static type is a struct (`Type::Obj`). When `o` is
//! `any`, the struct identity is only known at runtime, so SSA-lower
//! routes here: read the instance's `class_tag` (`u32` at `+8`), look
//! the layout up in the toolchain-emitted `__torajs_class_layouts`
//! table (via the `torajs-structmeta` Phase A4 helpers), and walk the
//! field metadata in declaration order.
//!
//! ## Scope — struct cells only (narrow surface)
//!
//! This is the `Tag::Obj` static-layout slot of the W-J substrate
//! trunk. Non-struct `any` values (dynobj / array / string /
//! primitive) reflect through different substrate and are **not**
//! claimed here: rather than silently returning an empty array (which
//! would be wrong for a dynobj or array that does have keys), a
//! non-struct cell throws a loud "not yet supported" — a separate
//! trunk, tracked in plan-state L3b.
//!
//! ## Anonymous-struct caveat (A1 MVP)
//!
//! An anonymous `{a: 1}` interned too late to receive a `class_tag`
//! carries `class_tag == 0`; `__torajs_struct_layout_lookup` returns
//! NULL and the walk yields an empty array. This matches Phase B's
//! `gOPD` returning `undefined` for the same instances — a shared,
//! documented A1 anon-stamp coverage gap, not a regression introduced
//! here.

use crate::reflect::{TAG_OBJ, heap_type_tag, is_cell_imm};
use core::ffi::{c_char, c_void};

unsafe extern "C" {
    // torajs-structmeta (W-J Phase A4) — read-side over __torajs_class_layouts.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;

    // torajs-arr — build the owned `Arr<Str>` result (8-byte slot).
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;

    // torajs-str — pooled key/name allocation.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    // torajs-throw — loud failure for the non-struct slot.
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// `(ptr, len)` field-name slice returned by
/// `__torajs_struct_field_name` — mirrors `torajs-structmeta::StrSlice`.
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance
/// (right after the 8-byte universal heap header).
const OBJ_CLASS_TAG_OFF: usize = 8;
/// `Str` payload offset (`[header:8][len u32 @8][_pad:4][bytes @16]`).
const STR_DATA_OFF: usize = 16;

/// Loud non-struct fallback message (NUL-terminated for the C throw).
const NON_STRUCT_MSG: &[u8] = b"Object.keys on a non-struct any value is not yet supported\0";

/// Allocate a pooled `Str` holding `len` bytes copied from `ptr`.
#[inline]
unsafe fn alloc_str_from_raw(ptr: *const u8, len: usize) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(len as u64) };
    if len != 0 && !ptr.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(ptr, s.add(STR_DATA_OFF), len) };
    }
    s
}

/// `Object.keys(v)` arm for a `Tag::Obj` struct cell. Returns an owned
/// (`+1`-rc) `Arr<Str>` of the struct's field names in declaration
/// order. A class with no layout (anonymous struct, `class_tag` 0 /
/// out of range) yields an empty array.
///
/// # Safety
/// `extern "C"` ABI. `v` is a NaN-box AnyValue immediate; for a struct
/// instance it is the raw cell pointer (cell-imm encoding). The caller
/// transfers the returned array's ownership into the SSA value flow and
/// runs a `throw_check` so the non-struct throw propagates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_struct_keys(v: u64) -> *mut c_void {
    // Narrow surface: only a live `Tag::Obj` struct cell is claimed.
    if !is_cell_imm(v) || unsafe { heap_type_tag(v as *const c_void) } != TAG_OBJ {
        unsafe { __torajs_throw_type_error(NON_STRUCT_MSG.as_ptr() as *const c_char) };
        return core::ptr::null_mut();
    }
    let cell = v as *const c_void;
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    // NULL layout → count 0 → empty array (A1 anon-stamp gap).
    let n = unsafe { __torajs_struct_field_count(layout) };

    let mut out = unsafe { __torajs_arr_alloc(n as u64) };
    let mut i = 0;
    while i < n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let s = unsafe { alloc_str_from_raw(name.ptr, name.len) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
        i += 1;
    }
    out as *mut c_void
}
