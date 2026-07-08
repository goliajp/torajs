//! `__torajs_any_member_get_tag` / `_value` — the tag-gated
//! `(tag, value)` probe behind arbitrary-name member reads on `any`
//! receivers (the read mirror of `member_set.rs`; RFC 20260704 C4+).
//!
//! Pre-gate the lowering's fallback handed the receiver's payload
//! bits straight to `__torajs_dynobj_get_tag/value`, reading every
//! cell as a DynObj layout — an Arr receiver's expando probe missed
//! by accident (silent `undefined`), any other tag was an
//! out-of-layout read. The pair below gates first:
//!
//! - null / undefined receiver → catchable TypeError (the tag call
//!   records it; the value call stays silent so the pair doesn't
//!   double-throw), pair answers `(ANY_UNDEF, 0)`.
//! - `Tag::DynObj` → the ordinary own-property probe, accessor
//!   sentinel included (the lowering's `emit_dynobj_get_result`
//!   consumes it unchanged).
//! - `Tag::Arr` → the `arrprops` expando probe (NULL props slot
//!   answers absent).
//! - `Tag::Closure` (L3b #11 residue, chunk 529) → the lazy
//!   `props_dynobj` at `CLOSURE_PROPS_OFF` (T-27 Function-as-Object
//!   expandos; NULL slot answers absent). `length` / `name` fn
//!   metadata stays a recorded boundary — the env cell carries no
//!   arity field, only the typed tier's compile-time fold sees it.
//! - every other receiver → `(ANY_UNDEF, 0)` — a definite absent,
//!   never a layout mis-read.
//!
//! The pair is borrow-shaped exactly like the dynobj probe it
//! wraps: the caller boxes via `any_box`, which takes its own
//! reference on heap payloads.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — expando probe through the props slot.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (mirror of `method_call_dynobj`'s declares).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// `class_tag` u32 offset inside a `Tag::Obj` instance / Str-cell
/// layout — mirrors `method_call_dynobj`'s constants.
const OBJ_CLASS_TAG_OFF: usize = 8;
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// The closure's `props_dynobj` pointer, NULL when no expando was
/// ever written.
unsafe fn closure_props(ptr: *mut c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void }
}

/// Cell tag of a dispatchable receiver, `None` for everything the
/// gate answers `(ANY_UNDEF, 0)` for.
fn recv_cell(recv: AnyValue) -> Option<(*mut c_void, u16)> {
    if !is_cell(recv) {
        return None;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a non-null encoded pointer; the
    // caller invariant says it points to a live heap object.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    Some((ptr, tag))
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_tag(recv: AnyValue, key: *const c_void) -> u64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 5;
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_get_tag(ptr, key) },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe { __torajs_arrprops_get_tag(ptr, key) },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if props.is_null() {
                5
            } else {
                __torajs_dynobj_get_tag(props, key)
            }
        },
        _ => 5,
    }
}

/// `o.m?.(…)` GetV-existence probe (chunk 709) — decides whether the
/// optional call's arguments evaluate. Returns 1 = the callee slot
/// resolves to a non-nullish value (or a plausibly-existing builtin
/// method): enter the call step; 0 = nullish / absent: short-circuit
/// to undefined (args never evaluate, per ES §13.3.9).
///
/// - null / undefined receiver → catchable TypeError (`o.m` itself
///   throws; the caller's throw-check propagates before branching).
/// - DynObj → own-property probe: present non-nullish (accessor
///   sentinel included) → 1; absent/nullish → 0 (a dynobj has no
///   builtin methods, so this is exact).
/// - Arr / Closure expandos → present non-nullish → 1; absent falls
///   through to the builtin test (an Arr's `push` is not an expando).
/// - struct cell (`Tag::Obj`) → class-layout field probe; found → 1;
///   miss falls through (harmless: the opt dispatcher's no-such arm
///   answers undefined).
/// - everything else → OPTIMISTIC: a known builtin method id enters
///   the call step (there is no per-arm support table); a wrong-arm
///   id lands on the opt dispatcher's no-such arm and answers
///   undefined. Residual: the args evaluate where ES would
///   short-circuit (`(42 as any).slice?.(f())` runs `f`) — recorded.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_probe(
    recv: AnyValue,
    mid: i64,
    key: *const c_void,
) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return 0;
    }
    let non_nullish = |tag: u64| (tag != 0 && tag != 5) as i64;
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            return non_nullish(unsafe { __torajs_dynobj_get_tag(ptr, key) });
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let tag = unsafe { __torajs_arrprops_get_tag(ptr, key) };
            if non_nullish(tag) == 1 {
                return 1;
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() && non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1
            {
                return 1;
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => {
            let class_tag =
                unsafe { (ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if !layout.is_null() {
                let name_len = unsafe { (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() };
                let name_bytes = unsafe { key.cast::<u8>().add(STR_DATA_OFF) };
                if unsafe { __torajs_struct_field_find(layout, name_bytes, name_len) } != u32::MAX {
                    return 1;
                }
            }
        }
        _ => {}
    }
    (mid != torajs_rc::ANY_METHOD_UNKNOWN) as i64
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_value(recv: AnyValue, key: *const c_void) -> u64 {
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { __torajs_dynobj_get_value(ptr, key) },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe { __torajs_arrprops_get_value(ptr, key) },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if props.is_null() {
                0
            } else {
                __torajs_dynobj_get_value(props, key)
            }
        },
        _ => 0,
    }
}
