//! `Tag::Closure` receiver arm for `__torajs_any_method_call`
//! (chunk 710) — `Function.prototype.call` / `apply` on closure
//! values that travel through the `any` world.
//!
//! The thisArg rides the closure's receiver channel: a recv-first
//! closure (RFC 20260717-objlit-anylane-recv — an any-lane literal
//! method whose body says `this`) takes it in argv[0], every other
//! closure body cannot reference `this` (class methods dispatch
//! through the vtable tier, never as bare closure cells) so the
//! thisArg drops:
//!
//! - `f.call(thisArg, a, b)` → `invoke_with_this(env, entry, thisArg, argv[1..])`.
//! - `f.apply(thisArg, list)` → the list unpacks per
//!   CreateListFromArrayLike (ES §7.3.19): `undefined` / `null` is
//!   an empty list; an `Arr` cell reads element-by-element through
//!   the kind-aware `__torajs_arr_index_get` (owned boxes, released
//!   after the call); anything else is a catchable TypeError.
//! - An expando property (chunk 529's lazy props bag) shadows the
//!   builtin per ES own-property order — `f.call = …` wins — and
//!   dispatches through the dynobj arm.
//! - A reified builtin method cell (chunk 711, `method_value`)
//!   short-circuits: `.call` / `.apply` re-dispatch the ORIGINAL
//!   method id with the thisArg as the receiver — `f.call(s)` where
//!   `f = s.toUpperCase` runs the string method on `s`. No receiver
//!   slot travels (a grow-relocating method reached through `.call`
//!   cannot write the caller's variable back — recorded boundary).
//! - `bind` (chunk 714) mints a bound-function cell (`method_bind`).
//! - A closure without a boxed dual entry cannot dispatch
//!   dynamically ([`not_callable`], same as the bare any-call lane).
//! - `toString` (RFC 20260719-fn-tostring-source B4) answers the
//!   type-erased source text from the fn-addr registry; reified
//!   builtin cells and source-less rows mint the JSC native form.
//! - Every other method id floats the no-such sentinel.
//!
//! Argument ledger: identical to the dispatcher — argv slots are
//! BORROWED; the apply unpacking's element boxes are this arm's own
//! temps and drop before returning.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_APPLY, ANY_METHOD_AT, ANY_METHOD_BIND, ANY_METHOD_CALL, ANY_METHOD_CONCAT,
    ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_SLICE,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, Tag,
};

use crate::method_call::{
    MAX_BOXED_ARGS, closure_cell_entry, invoke_with_this, method_no_such, not_callable,
};
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_short_str, is_undefined,
};

unsafe extern "C" {
    /// torajs-fnname — Function.prototype.toString kernels (RFC
    /// 20260719-fn-tostring-source B4): erased-source mint keyed on
    /// the registry fn_addr, and the named JSC native form for
    /// out-of-registry entries (reified builtin method cells).
    fn __torajs_fn_source_str(fn_addr: u64) -> *mut u8;
    fn __torajs_fn_native_form_str(name_ptr: *const u8, name_len: u32) -> *mut u8;
    /// torajs-dynobj — own-property probe (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — kind-aware boxed element read (owned +1 for
    /// cells; holes and OOB answer undefined).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — release an owned Str temp.
    fn __torajs_str_drop(s: *mut c_void);
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// `Tag::Closure` arm — see module doc.
///
/// # Safety
/// `ptr` is a valid `Tag::Closure` heap pointer; `argv` points at
/// `argc` AnyValue slots the caller keeps alive across the call;
/// `name_str` is NULL or a live Str cell.
pub(crate) unsafe fn closure_method(
    ptr: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // ES own-property order: an expando shadows the builtin.
        let props = *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void;
        if !props.is_null()
            && !name_str.is_null()
            && __torajs_dynobj_get_tag(props, name_str as *const c_void) != 5
        {
            // NULL recv_slot — the props side-table cell is not the
            // caller's variable (no relocation writeback target).
            return crate::method_call_dynobj::dynobj_method(
                props as *mut c_void,
                mid,
                name_str,
                core::ptr::null_mut(),
                argv,
                argc,
            );
        }
        match mid {
            m if m == ANY_METHOD_CALL => {
                let Some(target) = call_target(ptr) else {
                    return not_callable();
                };
                let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
                if argc <= 1 {
                    return dispatch(&target, this_arg, argv, 0);
                }
                dispatch(&target, this_arg, argv.add(1), argc - 1)
            }
            m if m == ANY_METHOD_APPLY => {
                let Some(target) = call_target(ptr) else {
                    return not_callable();
                };
                let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
                let list = if argc >= 2 {
                    *argv.add(1)
                } else {
                    VALUE_UNDEFINED
                };
                apply_list(&target, this_arg, list)
            }
            m if m == ANY_METHOD_BIND => crate::method_bind::bind_cell(ptr, argv, argc),
            // RFC 20260719-fn-tostring-source B4 —
            // Function.prototype.toString answers the type-erased
            // source text from the fn-addr registry; rows with no
            // recorded source (bound wrappers / reified builtin
            // cells whose fn_addr is the throwing native entry)
            // fall to the JSC native form inside the kernel.
            m if m == ANY_METHOD_TO_STRING => {
                if let Some(name) = crate::method_value::builtin_method_name(ptr) {
                    // A reified builtin method cell is never in the
                    // registry — mint the named native form directly.
                    return crate::nanbox::box_void_ptr(__torajs_fn_native_form_str(
                        name.as_ptr(),
                        name.len() as u32,
                    ) as *mut c_void);
                }
                // B6c — a class-method face resolves its adapter's
                // registry row (the erased method-shorthand source).
                let fn_addr = crate::method_value_class::registry_addr(ptr);
                crate::nanbox::box_void_ptr(__torajs_fn_source_str(fn_addr) as *mut c_void)
            }
            _ => method_no_such(),
        }
    }
}

/// What `.call` / `.apply` re-invokes — a closure's boxed dual
/// entry (the thisArg rides the receiver channel: a recv-first
/// closure takes it in argv[0], a plain closure drops it — RFC
/// 20260717-objlit-anylane-recv knife 2d), or a reified builtin
/// method's original id re-dispatched with the thisArg as the
/// receiver (chunk 711).
enum CallTarget {
    Boxed(*mut c_void, u64),
    Builtin(i64),
    /// A reified class method / accessor face (RFC
    /// 20260718-accessor-reify 刀 2) — the carried adapter invokes
    /// with the thisArg in the env slot.
    ClassAdapter(u64),
}

/// Classify the receiver closure cell.
unsafe fn call_target(ptr: *mut c_void) -> Option<CallTarget> {
    unsafe {
        if let Some(target_mid) = crate::method_value::builtin_method_mid(ptr) {
            return Some(CallTarget::Builtin(target_mid));
        }
        if let Some(adapter) = crate::method_value_class::class_method_adapter(ptr) {
            return Some(CallTarget::ClassAdapter(adapter));
        }
        closure_cell_entry(ptr).map(|(env, entry)| CallTarget::Boxed(env, entry))
    }
}

/// Invoke the target with an unpacked argument window. The builtin
/// re-dispatch passes no name bytes and no receiver slot (a grow-
/// relocating method reached through `.call` cannot write the
/// caller's variable back — recorded boundary).
unsafe fn dispatch(
    target: &CallTarget,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        match target {
            CallTarget::Boxed(env, entry) => invoke_with_this(*env, *entry, this_arg, argv, argc),
            CallTarget::ClassAdapter(adapter) => {
                crate::method_value_class::__torajs_class_face_invoke(
                    *adapter, this_arg, argv, argc,
                )
            }
            CallTarget::Builtin(mid) => {
                if let Some(out) = generic_str_this(*mid, this_arg, argv, argc) {
                    return out;
                }
                crate::method_call::any_method_call_inner(
                    this_arg,
                    *mid,
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    argv,
                    argc,
                )
            }
        }
    }
}

/// §22.1.3 "the String.prototype methods are generic" — a reified
/// String.prototype method reached through `.call` / `.apply` with a
/// non-string thisArg runs ToString(this) (full OrdinaryToPrimitive,
/// observable toString→valueOf order; a double-object receiver
/// leaves a pending TypeError for the caller's throw check) and
/// dispatches the Str arm on the coerced temp. Excluded from the
/// coerce, staying on the ordinary lane:
/// - `toString` / `valueOf` (thisStringValue §22.1.3.28/.35) and
///   `toLocaleString` — a non-String receiver is a TypeError there;
/// - mids SHARED with the Array surface (at / concat / includes /
///   indexOf / lastIndexOf / slice) — a reified cell carries only
///   its mid, no family, so `Array.prototype.indexOf.call(arrayLike)`
///   re-dispatches the same id and must reach the array-like generic
///   arm (per-family cells are the recorded fix for the String half);
/// - string-shaped and nullish receivers (identity fast paths /
///   RequireObjectCoercible throw).
pub(crate) unsafe fn generic_str_this(
    mid: i64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if !crate::method_support::str_supports(mid)
        || matches!(
            mid,
            ANY_METHOD_TO_STRING
                | ANY_METHOD_VALUE_OF
                | ANY_METHOD_TO_LOCALE_STRING
                | ANY_METHOD_AT
                | ANY_METHOD_CONCAT
                | ANY_METHOD_INCLUDES
                | ANY_METHOD_INDEX_OF
                | ANY_METHOD_LAST_INDEX_OF
                | ANY_METHOD_SLICE
        )
    {
        return None;
    }
    if is_undefined(this_arg) || is_null(this_arg) || is_short_str(this_arg) {
        return None;
    }
    if is_cell(this_arg) {
        let ptr = as_void_ptr(this_arg);
        if unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() } == Tag::Str as u16 {
            return None;
        }
    }
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(this_arg);
        let out = crate::method_call_str::str_method(s as *mut u8, mid, argv, argc);
        __torajs_str_drop(s);
        Some(out)
    }
}

/// CreateListFromArrayLike + invoke — `undefined` / `null` is an
/// empty list, an `Arr` cell unpacks element-by-element, everything
/// else is a catchable TypeError.
unsafe fn apply_list(target: &CallTarget, this_arg: AnyValue, list: AnyValue) -> AnyValue {
    unsafe {
        if is_undefined(list) || is_null(list) {
            return dispatch(target, this_arg, core::ptr::null(), 0);
        }
        let arr = if is_cell(list) {
            as_void_ptr(list)
        } else {
            core::ptr::null_mut()
        };
        if arr.is_null() || (arr.cast::<u8>().add(4) as *const u16).read() != Tag::Arr as u16 {
            __torajs_throw_type_error(
                c"second argument to Function.prototype.apply must be an array".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let n = *(arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64) as usize;
        // Element boxes are owned temps (+1 per cell) — released
        // after the invoke; the common small shape stays on the
        // stack, mirror of `invoke_boxed`'s own buffer split.
        let out;
        if n > MAX_BOXED_ARGS {
            let boxed: Vec<u64> = (0..n)
                .map(|i| __torajs_arr_index_get(arr, i as i64))
                .collect();
            out = dispatch(target, this_arg, boxed.as_ptr(), n as i64);
            for b in boxed {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(b);
            }
        } else {
            let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
            for (i, slot) in buf.iter_mut().enumerate().take(n) {
                *slot = __torajs_arr_index_get(arr, i as i64);
            }
            out = dispatch(target, this_arg, buf.as_ptr(), n as i64);
            for b in buf.iter().take(n) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(*b);
            }
        }
        out
    }
}
