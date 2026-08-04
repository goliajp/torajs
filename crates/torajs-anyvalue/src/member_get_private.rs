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
use crate::struct_probe::struct_field_pair;
use core::ffi::c_void;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
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
    let tag = unsafe { __torajs_any_member_get_tag(recv, key) };
    if tag != AnySlotTag::Undef as u64 {
        return tag;
    }
    // The base walk already threw for a null / undefined receiver —
    // don't stack a second TypeError on the pending one.
    if is_null(recv) || is_undefined(recv) {
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
            __torajs_throw_type_error(
                c"cannot read private member from an object whose class did not declare it"
                    .as_ptr(),
            );
        }
    }
    tag
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
