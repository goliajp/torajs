//! `__torajs_any_member_set_priv` — the brand-gated write channel a
//! `#x` member WRITE lowers to (spec §7.3.32 PrivateSet: writing a
//! private element to an object whose class did not declare it
//! throws TypeError, never installs an expando).
//!
//! The lowering picks this channel STATICALLY (`__priv_` prefix on
//! the member name in `emit_any_member_set`), so the ordinary member
//! write hot path pays nothing. Unlike the read twin
//! (`member_get_private`) this shell must gate BEFORE deferring —
//! the base kernel's miss tail happily installs an expando entry,
//! which is exactly the silent-wrong PrivateSet forbids. A declared
//! brand (layout field / expando on a degraded instance / dynobj
//! entry / prototype-face accessor) defers to the base kernel, whose
//! accessor dispatch runs a private setter and raises the readonly
//! TypeError for a getter-only pair. Primitives and nullish
//! receivers carry no brand and throw here (PrivateSet's ToObject-
//! free receiver check).

use crate::member_get_layout::{recv_cell, struct_props};
use crate::member_get_private::priv_brand_declared;
use crate::nanbox::AnyValue;
use crate::struct_probe::{ANY_ACCESSOR_TAG, struct_accessor_key, struct_field_pair};
use core::ffi::c_void;
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_any_member_set(
        recv_slot: *mut AnyValue,
        key: *mut c_void,
        tag: u64,
        value: u64,
        hint: i64,
    );
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_accessor_get_setter(pair: *const c_void) -> *mut c_void;
}

/// See module doc.
///
/// # Safety
/// `recv_slot` points at a live AnyValue slot; `key` is a live Str
/// cell (private names are never symbols).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set_priv(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
) {
    let recv = unsafe { *recv_slot };
    // Slice 2c — an element held in the foreign store takes the write
    // there; the store takes its own stake, so the handed-in one ends.
    let boxed =
        unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64) };
    if unsafe { crate::private_foreign::foreign_set(recv, key, boxed) } {
        if tag == torajs_rc::AnySlotTag::Heap as u64 && value != 0 {
            unsafe { __torajs_value_drop_heap(value as *mut c_void) };
        }
        return;
    }
    if !unsafe { priv_brand_declared(recv, key) } {
        unsafe {
            __torajs_throw_type_error(
                c"cannot write private member to an object whose class did not declare it".as_ptr(),
            );
        }
        return;
    }
    // §7.3.32 step 4 — a private METHOD is not writable. Of the brand
    // faces above only a class method is neither a layout field, an
    // expando entry nor an accessor (those three stay writable, or
    // throw their own readonly TypeError in the base kernel).
    if let Some((ptr, t)) = recv_cell(recv)
        && t == Tag::Obj as u16
        && unsafe { private_method_face(ptr, key) }
    {
        unsafe { __torajs_throw_type_error(c"private method is not writable".as_ptr()) };
        return;
    }
    // §7.3.32 step 5.c — an accessor without a setter throws whatever
    // the strictness; a static one is an own AccessorPair entry on the
    // class object (a dynobj), whose base set only refuses loudly in
    // strict callers.
    if let Some((ptr, t)) = recv_cell(recv)
        && t == Tag::DynObj as u16
        && unsafe { __torajs_dynobj_get_tag(ptr, key) } == ANY_ACCESSOR_TAG
        && unsafe {
            __torajs_accessor_get_setter(__torajs_dynobj_get_value(ptr, key) as *const c_void)
        }
        .is_null()
    {
        unsafe {
            __torajs_throw_type_error(c"private accessor was defined without a setter".as_ptr())
        };
        return;
    }
    unsafe { __torajs_any_member_set(recv_slot, key, tag, value, hint) }
}

/// Whether a brand-declared private `key` on the struct cell `ptr`
/// names a method — see the write gate above.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `key` a live Str cell.
unsafe fn private_method_face(ptr: *mut c_void, key: *mut c_void) -> bool {
    unsafe {
        if struct_field_pair(ptr, key).is_some() || struct_accessor_key(ptr, key) {
            return false;
        }
        let props = struct_props(ptr);
        props.is_null() || __torajs_dynobj_has(props, key) == 0
    }
}
