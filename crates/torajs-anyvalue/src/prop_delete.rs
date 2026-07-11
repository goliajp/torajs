//! `__torajs_any_prop_delete` — `delete recv.k` / `delete recv[k]`
//! on an `any` receiver (ES §13.5.1 / §10.1.10 OrdinaryDelete).
//!
//! Per-receiver dispatch mirrors the `member_get` gate:
//!
//! - null / undefined receiver → catchable TypeError (§13.5.1.2
//!   evaluates the property reference first; ToObject throws),
//!   answers 0 — the lowering's throw-check propagates before the
//!   value is consumed.
//! - `Tag::DynObj` → `__torajs_dynobj_delete` (drops the entry's
//!   key + heap value, tombstones the slot), answers 1 regardless —
//!   an absent key deletes to true per spec, and a dynobj has no
//!   non-configurable properties.
//! - `Tag::Arr` / `Tag::Closure` → expando delete through the props
//!   dynobj (NULL props slot = absent = true).
//! - `Tag::Obj` (struct cell) → 0. A fixed class layout has no
//!   removable slots; answering false is the honest spelling of
//!   "not configurable" (recorded divergence: bun deletes and
//!   answers true — structs-through-any are a reflection boundary).
//! - every other receiver (Str / Num / Bool / boxed primitives) →
//!   1: `delete` on a non-object base answers true (§13.5.1.2 —
//!   the property reference never materializes an own property).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, recv_cell};
use crate::nanbox::{AnyValue, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — OrdinaryDelete (1 = an entry was removed).
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando delete through the props slot.
    fn __torajs_arrprops_delete(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// See module doc. `key` is a live Str cell (the lowering interns
/// static names and materializes dynamic string keys before the
/// call).
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_delete(recv: AnyValue, key: *const c_void) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot delete a property of null or undefined".as_ptr());
        }
        return 0;
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            unsafe { __torajs_dynobj_delete(ptr, key) };
            1
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            unsafe { __torajs_arrprops_delete(ptr, key) };
            1
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() {
                unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
            }
            1
        }
        Some((_, t)) if t == Tag::Obj as u16 => 0,
        _ => 1,
    }
}
