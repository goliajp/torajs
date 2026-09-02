//! `__torajs_any_method_probe` — the optional-call GetV-existence
//! probe, split from `member_get.rs` (file-size cap). Shares the
//! parent module's receiver-shape helpers; the borrow/absent
//! contract is documented there.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, is_wrapper_tag, recv_cell, wrapper_props};
use crate::nanbox::{AnyValue, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe (5 = absent) + existence.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando twins.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-structmeta — layout probe pair (see parent module).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Struct-cell class-tag slot — mirror of the parent module's.
const OBJ_CLASS_TAG_OFF: usize = 8;

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
///   miss falls through to the support table (a struct has no
///   builtin methods, so the miss short-circuits to undefined).
/// - everything else → the exact per-receiver-shape support table
///   (chunk 711's `builtin_method_supported`): a supported id
///   enters the call step; a wrong-arm id short-circuits to
///   undefined without evaluating the arguments (chunk 713 —
///   closes 709's recorded residual where `(42 as any).slice?.(f())`
///   ran `f`).
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
            // The whole GetV, not just the own slot. `dynobj_arm_tag`
            // is the read the dispatcher itself performs — own entry,
            // stored-undefined shadow, `__proto__`, the user
            // [[Prototype]] chain, a null-proto cut, and finally the
            // builtin reify surface. Answering from
            // `__torajs_dynobj_get_tag` alone was exact only while a
            // dynobj had nothing above it; once %Object.prototype%
            // was on the chain (521 knives 4-5), `o.hasOwnProperty?.()`
            // and every method reached through `Object.create(p)`
            // short-circuited to undefined here, in front of a
            // dispatcher that would have resolved them.
            return non_nullish(unsafe { crate::member_get::dynobj_arm_tag(ptr, recv, key) });
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let tag = unsafe { __torajs_arrprops_get_tag(ptr, key) };
            if non_nullish(tag) == 1 {
                return 1;
            }
            // Stored-undefined expando shadows the builtin — the
            // optional call short-circuits (`arr.join = undefined;
            // arr.join?.()` is undefined, not a reified join).
            if tag == 5 && unsafe { __torajs_arrprops_has(ptr, key) } != 0 {
                return 0;
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() {
                if non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1 {
                    return 1;
                }
                // Stored-undefined shadow — see the Arr arm.
                if unsafe { __torajs_dynobj_has(props, key) } != 0 {
                    return 0;
                }
            }
        }
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper own-
        // property expando probe. Miss falls through to the shared
        // `builtin_method_supported` table below (mirror of the
        // Arr / Closure fall-through — a wrapper's inherited
        // `.toString` / `.valueOf` etc. surface reifies there).
        Some((ptr, t)) if is_wrapper_tag(t) => {
            let props = unsafe { wrapper_props(ptr) };
            if !props.is_null() {
                if non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1 {
                    return 1;
                }
                // Stored-undefined shadow — see the Arr arm.
                if unsafe { __torajs_dynobj_has(props, key) } != 0 {
                    return 0;
                }
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => {
            // Blade 2 — an expando entry answers the existence probe
            // like any own property.
            let props = unsafe { crate::member_get_layout::struct_props(ptr) };
            if !props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0 {
                return 1;
            }
            let class_tag =
                unsafe { (ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if !layout.is_null() {
                let k = unsafe { crate::key_wtf8::KeyWtf8::of(key) };
                if unsafe { __torajs_struct_field_find(layout, k.as_ptr(), k.len()) } != u32::MAX {
                    return 1;
                }
            }
        }
        _ => {}
    }
    // chunk 713 — exact per-receiver-shape support table (chunk
    // 711's reification table) instead of the optimistic known-id
    // test: a wrong-arm name short-circuits to undefined WITHOUT
    // evaluating the arguments (`(42 as any).slice?.(f())` no
    // longer runs `f`, closing chunk 709's recorded residual).
    crate::method_value::builtin_method_supported(recv, mid) as i64
}
