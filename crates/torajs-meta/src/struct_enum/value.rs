//! The value one own property contributes to the `Object.values` /
//! `Object.entries` walks, moved out of [`super`] whole.

use super::{__torajs_struct_field_info, StrSlice};
use crate::struct_reflect::{accessor_slot_name, field_slot_to_pair_owned};
use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-anyvalue — a struct accessor's [[Get]], keyed by raw name
    /// bytes (RFC 20260714-objlit-accessor blade 6). Answers the
    /// getter's result OWNED; `undefined` for a set-only property.
    fn __torajs_struct_accessor_get(obj: *mut c_void, name: *const u8, name_len: u32) -> u64;
    /// torajs-anyvalue — §10.1.8.1 [[Get]] of a declared field, owned.
    fn __torajs_obj_field_get_exotic_impl(p: *mut c_void, name: *const c_void) -> u64;
    /// torajs-anyvalue — split an AnyValue into its `(tag, value)` pair.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `torajs_rc::FLAG_OBJ_EXOTIC_FIELD` mirror (the u16 flags at `+6`).
const OBJ_HDR_FLAG_EXOTIC_FIELD: u16 = 1 << 15;

/// An AnyValue answer as its `(tag, value)` pair, the stake moving
/// with it.
unsafe fn unbox(boxed: u64) -> (u64, u64) {
    unsafe {
        (
            __torajs_anyv_unbox_tag(boxed) as u64,
            __torajs_anyv_unbox_value(boxed) as u64,
        )
    }
}

/// The `(tag, value)` an own property contributes to `Object.values` /
/// `Object.entries`, OWNED (the caller pushes it into the array, which
/// takes the ref). RFC 20260714-objlit-accessor blade 6 — an accessor
/// slot contributes the getter's RESULT ([[Get]], ES §7.3.25 / §20.1.2.x),
/// not the getter closure sitting in the slot; the walkers used to hand
/// the closure over (`[1, [Function: __getter_v]]` where bun says
/// `[1, 2]`). The getter's result arrives owned, so unlike the borrowed
/// field slot below it takes NO rc_inc.
///
/// # Safety
/// `cell` is a live `Tag::Obj` struct cell; `layout` its live entry.
pub(super) unsafe fn slot_own_value(
    cell: *const c_void,
    layout: *const c_void,
    idx: u32,
    name: StrSlice,
) -> (u64, u64) {
    if let Some((_, p, plen)) = unsafe { accessor_slot_name(name.ptr, name.len) } {
        let boxed = unsafe { __torajs_struct_accessor_get(cell as *mut c_void, p, plen as u32) };
        return unsafe { unbox(boxed) };
    }
    // a declared field `Object.defineProperty` redefined may be an
    // accessor now: its value is what [[Get]] answers
    if unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() } & OBJ_HDR_FLAG_EXOTIC_FIELD != 0 {
        let key = unsafe { super::alloc_str_from_raw(name.ptr, name.len) };
        let boxed = unsafe {
            __torajs_obj_field_get_exotic_impl(cell as *mut c_void, key as *const c_void)
        };
        unsafe { super::__torajs_value_drop_heap(key as *mut c_void) };
        return unsafe { unbox(boxed) };
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        cell.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    // RFC 20260915-declared-field-shadow — a shadowed field reads as
    // an Any slot from its sidecar.
    let (type_tag, raw) = unsafe {
        crate::struct_field_attrs::slot_or_shadow(cell, name.ptr, name.len, info.type_tag, raw)
    };
    // Owned pair decode — exactly one fresh stake per heap payload
    // (cell +1 / an Any slot's ShortStr materializes at rc=1). The
    // pre-fix borrow-pair + rc_inc left a ShortStr's materialized
    // Str at rc=2 with one reclaiming drop — 32B leaked per
    // `Object.values` read of the slot.
    unsafe { field_slot_to_pair_owned(type_tag, raw) }
}
