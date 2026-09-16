//! `__torajs_any_member_get_priv_tag` — the brand-gated tag channel a
//! `#x` member READ lowers to (spec §7.3.31 PrivateGet / PrivateBrandCheck:
//! reading a private element from an object whose class did not
//! declare it throws TypeError, never answers `undefined`).
//!
//! The lowering picks this channel STATICALLY (`__priv_` prefix on
//! the member name in `emit_member_fallback`), so the ordinary member
//! hot path pays nothing. The shell defers to the base tag walk and
//! only disambiguates the `undefined` answer: a declared field
//! STORING undefined (the layout face or the +24 expando dict on a
//! degraded instance carries the name) keeps it; a genuine own-face
//! miss is the foreign-brand read and throws. The paired value call
//! stays the base `__torajs_any_member_get_value` — on a throw it
//! runs before the caller's throw-check sees the pending state, and
//! answers 0 without a second throw (the null-receiver convention).

use crate::member_get::__torajs_any_member_get_tag;
use crate::member_get_layout::{recv_cell, struct_props};
use crate::nanbox::{AnyValue, is_null, is_undefined};
use crate::struct_probe::{ANY_ACCESSOR_TAG, KIND_GETTER, resolve, struct_field_pair};
use core::ffi::c_void;
use torajs_rc::str_wtf8::StrWtf8;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_accessor_get_getter(pair: *const c_void) -> *mut c_void;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell
/// (private names are never symbols).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_priv_tag(
    recv: AnyValue,
    key: *const c_void,
) -> u64 {
    // Slice 2c — an element installed on an object its class did not
    // mint lives in the foreign store, Proxy receivers included.
    if let Some(v) = unsafe { crate::private_foreign::foreign_get(recv, key) } {
        return crate::nanbox_encode::__torajs_anyv_unbox_tag(v) as u64;
    }
    let tag = unsafe { __torajs_any_member_get_tag(recv, key) };
    // The base walk already threw for a null / undefined receiver —
    // don't stack a second TypeError on the pending one.
    if is_null(recv) || is_undefined(recv) {
        return tag;
    }
    // §7.3.31 step 1 — a Proxy has its own, always empty, private
    // elements: the base walk answers the accessor sentinel for it
    // (the `get` trap route), which must never reach the target.
    if matches!(recv_cell(recv), Some((_, t)) if t == Tag::Proxy as u16) {
        unsafe { throw_private(c"cannot read private member from a Proxy") };
        return AnySlotTag::Undef as u64;
    }
    if tag == ANY_ACCESSOR_TAG {
        // §7.3.31 step 6 — an accessor whose [[Get]] is undefined
        // throws; the ordinary member channel answers `undefined`
        // for the same shape, which is right for public names only.
        if let Some((ptr, t)) = recv_cell(recv)
            && t == Tag::Obj as u16
            && unsafe { resolve(ptr, StrWtf8::of(key).as_bytes(), KIND_GETTER) }.is_none()
        {
            unsafe { throw_private(c"private accessor was defined without a getter") };
            return AnySlotTag::Undef as u64;
        }
        // A static private accessor lives as an own AccessorPair entry
        // on the class object (a dynobj).
        if let Some((ptr, t)) = recv_cell(recv)
            && t == Tag::DynObj as u16
            && unsafe { __torajs_dynobj_get_tag(ptr, key) } == ANY_ACCESSOR_TAG
            && unsafe {
                __torajs_accessor_get_getter(__torajs_dynobj_get_value(ptr, key) as *const c_void)
            }
            .is_null()
        {
            unsafe { throw_private(c"private accessor was defined without a getter") };
            return AnySlotTag::Undef as u64;
        }
        return tag;
    }
    if tag != AnySlotTag::Undef as u64 {
        return tag;
    }
    let declared = match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            struct_field_pair(ptr, key).is_some() || {
                let props = struct_props(ptr);
                !props.is_null() && __torajs_dynobj_has(props, key) != 0
            }
        },
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_has(ptr, key) != 0 },
        // Primitives / other cell shapes never carry a private brand.
        _ => false,
    };
    if !declared {
        unsafe {
            throw_private(
                c"cannot read private member from an object whose class did not declare it",
            )
        };
    }
    tag
}

unsafe fn throw_private(msg: &core::ffi::CStr) {
    unsafe { __torajs_throw_type_error(msg.as_ptr()) }
}

/// ES2022 §13.10.1 ergonomic brand check (`#x in o`) — whether the
/// receiver's OWN face carries the private element: a declared layout
/// field, a degraded-instance expando entry, a dynobj entry, or a
/// present value / accessor through the base tag walk (a private
/// METHOD reifies on the class prototype face, which that walk
/// resolves). A non-Object rhs throws TypeError per step 5 and
/// answers false under the pending throw.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_priv_has(recv: AnyValue, key: *const c_void) -> bool {
    // §13.10.1 step 5 — a non-Object rhs (immediates + the heap
    // primitives) throws; every OTHER object shape (closure, array,
    // map, wrapper …) is a legal rhs that simply carries no brand.
    if unsafe { torajs_rc::in_op_any::require_object_rhs(recv as i64) }.is_none() {
        return false;
    }
    unsafe { priv_brand_declared(recv, key) || crate::private_foreign::foreign_has(recv, key) }
}

/// The value twin of [`__torajs_any_member_get_priv_tag`]: a foreign
/// store element's value, borrowed; otherwise the base value channel.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_priv_value(
    recv: AnyValue,
    key: *const c_void,
) -> u64 {
    if let Some(v) = unsafe { crate::private_foreign::foreign_get(recv, key) } {
        return crate::nanbox_encode::__torajs_anyv_unbox_value(v) as u64;
    }
    unsafe { crate::member_get_value::__torajs_any_member_get_value(recv, key) }
}

/// Whether the receiver's OWN face carries the private element — the
/// shared brand truth behind `#x in o` above and the write channel's
/// gate (`member_set_private`): a declared layout field, a
/// degraded-instance expando entry, a dynobj entry, or a present
/// value / accessor through the base tag walk (a private METHOD or
/// accessor reifies on the class prototype face, which that walk
/// resolves). Primitives and nullish never carry a brand.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
pub(crate) unsafe fn priv_brand_declared(recv: AnyValue, key: *const c_void) -> bool {
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            struct_field_pair(ptr, key).is_some()
                || {
                    let props = struct_props(ptr);
                    !props.is_null() && __torajs_dynobj_has(props, key) != 0
                }
                || __torajs_any_member_get_tag(recv, key) != AnySlotTag::Undef as u64
        },
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_has(ptr, key) != 0 },
        _ => false,
    }
}
