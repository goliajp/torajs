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

use crate::member_get::{closure_props, header_flag_set, recv_cell};

/// torajs-arr inline props-dynobj slot at +24
/// (`torajs_arr::layout::ARR_PROPS_OFF` mirror — same constant
/// `prop_has` uses).
unsafe fn arr_props(arr: *mut c_void) -> *const c_void {
    unsafe { arr.cast::<u8>().add(24).cast::<*const c_void>().read() }
}
use crate::nanbox::{AnyValue, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — OrdinaryDelete (1 = an entry was removed).
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — own-entry presence + packed W/E/C flags
    /// (bit 2 = configurable) for the §10.1.10 step-4 refusal.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
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
            if unsafe { refuse_non_configurable(ptr, key) } {
                return 0;
            }
            unsafe { __torajs_dynobj_delete(ptr, key) };
            // RFC 20260712 chunk 3 — a builtin `<Ctor>.prototype`
            // singleton also hides its interned family method behind
            // the deleted-mid tombstone (the entry delete above only
            // removes a monkey-patch shadow, if any). Idempotent.
            let proto_tag =
                unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
            if proto_tag >= 0 {
                let mid = unsafe { crate::method_value::key_method_id(key) };
                if mid != torajs_rc::ANY_METHOD_UNKNOWN
                    && crate::method_support::proto_tag_family_owns(proto_tag, mid)
                {
                    unsafe {
                        torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(
                            proto_tag, mid,
                        )
                    };
                }
            }
            1
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let props = unsafe { arr_props(ptr) };
            if !props.is_null() && unsafe { refuse_non_configurable(props, key) } {
                return 0;
            }
            unsafe { __torajs_arrprops_delete(ptr, key) };
            1
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() {
                if unsafe { refuse_non_configurable(props as *mut c_void, key) } {
                    return 0;
                }
                unsafe { __torajs_dynobj_delete(props as *mut c_void, key) };
            }
            // chunk C — the virtual §20.2.4 name/length pair is
            // configurable: a delete tombstones the header bit so
            // every reader skips the virtual answer (idempotent on
            // a re-delete / post-recreate delete).
            if unsafe { crate::prop_has::key_is(key, b"name") } {
                unsafe { header_flag_set(ptr, torajs_rc::FLAG_FN_NAME_DELETED) };
            }
            if unsafe { crate::prop_has::key_is(key, b"length") } {
                unsafe { header_flag_set(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED) };
            }
            1
        }
        Some((_, t)) if t == Tag::Obj as u16 => 0,
        _ => 1,
    }
}

/// §10.1.10 OrdinaryDelete step 4 (chunk D-2a, RFC 20260711): a
/// present own property whose `configurable` flag is clear refuses
/// the delete; tr programs are module-strict so §13.5.1.2 then
/// throws the catchable TypeError (test262 propertyHelper's
/// isConfigurable probe relies on exactly this shape). A plain
/// `o.k = v` entry carries BUCKET_FLAGS_DEFAULT (all set), so only
/// defineProperty-shaped entries can refuse.
unsafe fn refuse_non_configurable(obj: *const c_void, key: *const c_void) -> bool {
    if unsafe { __torajs_dynobj_has(obj, key) } != 0
        && unsafe { __torajs_dynobj_get_flags(obj, key) } & 0x4 == 0
    {
        unsafe {
            __torajs_throw_type_error(c"cannot delete a non-configurable property".as_ptr());
        }
        return true;
    }
    false
}
