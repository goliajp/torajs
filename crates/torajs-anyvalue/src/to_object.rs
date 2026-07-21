//! `__torajs_any_to_object` — `Object(v)` callable coercion per ES
//! §20.1.1.1 / ToObject §7.1.18. RFC 20260716-primitive-wrapper-
//! substrate 刀 4.
//!
//! Per-tag mapping (owned return, +1 to caller):
//! - `null` / `undefined` → fresh empty DynObj (`__torajs_dynobj_alloc`)
//! - `int32` / `double` → fresh `NumberWrapper([[NumberData]] = f64)`
//! - `bool` → fresh `BooleanWrapper([[BooleanData]] = 0|1)`
//! - `short_str` → materialize + fresh `StringWrapper([[StringData]] =
//!   new heap Str)`
//! - `Tag::Str` cell → fresh `StringWrapper([[StringData]] = recv +1)`
//!   (wrapper adopts a fresh reference on the inner cell; the input
//!   AnyValue's +1 keeps ownership so the caller still drops it)
//! - `Tag::Symbol` cell → fresh `SymbolWrapper([[SymbolData]] =
//!   recv +1)` — Symbol is a primitive per §7.1.18, so `typeof
//!   Object(sym)` answers "object" with disjoint identity
//! - any other heap cell (Arr / Obj / DynObj / Closure / Map / Set /
//!   Date / RegExp / Promise / Wrapper / ...) → identity per ToObject
//!   step-3 (`rc_inc(recv)`, return recv)
//!
//! Non-identity for primitives is what makes `Object(5) === Object(5)`
//! answer `false` — each call mints a fresh wrapper cell whose heap
//! identity is disjoint from every other. Heap receivers keep identity
//! so `Object(obj) === obj` per spec.

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, Tag};

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, box_void_ptr, is_bool, is_cell, is_double,
    is_int32, is_null, is_short_str, is_undefined,
};
use crate::nanbox_ffi_materialize::materialize_short_str;

unsafe extern "C" {
    /// torajs-wrapper — Number wrapper alloc (rc=1; `[[NumberData]]`).
    fn __torajs_number_wrapper_new(val: f64) -> *mut u8;
    /// torajs-wrapper — String wrapper alloc (transfer +1 on cell).
    fn __torajs_string_wrapper_new(cell: *mut u8) -> *mut u8;
    /// torajs-wrapper — Boolean wrapper alloc (rc=1; `[[BooleanData]]`).
    fn __torajs_boolean_wrapper_new(val: u8) -> *mut u8;
    /// torajs-wrapper — Symbol wrapper alloc (transfer +1 on cell).
    fn __torajs_symbol_wrapper_new(cell: *mut u8) -> *mut u8;
    /// torajs-dynobj — fresh empty dynobj (rc=1) — the null/undef arm.
    fn __torajs_dynobj_alloc() -> *mut c_void;
}

/// See module doc.
///
/// # Safety
/// Cell operands point to live heap objects matching their header tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_object(v: AnyValue) -> AnyValue {
    // Step 1 — `Object(null)` / `Object(undefined)` → OrdinaryObjectCreate.
    if is_null(v) || is_undefined(v) {
        let ptr = unsafe { __torajs_dynobj_alloc() };
        return box_void_ptr(ptr);
    }
    // Primitives → fresh wrapper (each call mints its own identity).
    if is_int32(v) {
        let w = unsafe { __torajs_number_wrapper_new(as_int32(v) as f64) };
        return box_void_ptr(w as *mut c_void);
    }
    if is_double(v) {
        let w = unsafe { __torajs_number_wrapper_new(as_double(v)) };
        return box_void_ptr(w as *mut c_void);
    }
    if is_bool(v) {
        let w = unsafe { __torajs_boolean_wrapper_new(if as_bool(v) { 1 } else { 0 }) };
        return box_void_ptr(w as *mut c_void);
    }
    if is_short_str(v) {
        let cell = unsafe { materialize_short_str(v) };
        let w = unsafe { __torajs_string_wrapper_new(cell) };
        return box_void_ptr(w as *mut c_void);
    }
    // Heap cell — Str tag wraps into StringWrapper, everything else is
    // identity per ToObject step 3 (an object is already an object).
    if is_cell(v) {
        let ptr = as_void_ptr(v);
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::Str as u16 {
            // Wrapper adopts a fresh +1 on the inner cell; caller
            // keeps its own +1 on the input.
            unsafe { __torajs_rc_inc(ptr) };
            let w = unsafe { __torajs_string_wrapper_new(ptr as *mut u8) };
            return box_void_ptr(w as *mut c_void);
        }
        // Symbol is a primitive too (§7.1.18 step for Symbol) —
        // `Object(sym)` mints a fresh SymbolWrapper so `typeof`
        // answers "object" and identity is disjoint from `sym`.
        if tag == Tag::Symbol as u16 {
            unsafe { __torajs_rc_inc(ptr) };
            let w = unsafe { __torajs_symbol_wrapper_new(ptr as *mut u8) };
            return box_void_ptr(w as *mut c_void);
        }
        // Identity — hand the caller a fresh +1 on the same cell.
        unsafe { __torajs_rc_inc(ptr) };
        return v;
    }
    // Unreachable: every AnyValue shape above answers. If a future
    // encoding lands, defensively fresh-object it so no cell leaks.
    box_void_ptr(unsafe { __torajs_dynobj_alloc() })
}
