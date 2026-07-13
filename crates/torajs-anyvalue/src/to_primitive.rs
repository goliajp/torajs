//! OrdinaryToPrimitive (ES §7.1.1.1) for heap-object Any operands.
//!
//! `ToString(obj)` / `ToNumber(obj)` must run the object's own (or
//! monkey-patched) `toString` / `valueOf` in hint order and accept
//! the FIRST primitive result — where "primitive" includes
//! `undefined` and `null` (`String({toString(){}})` is
//! `"undefined"`, not `"[object Object]"`). Only when both methods
//! answer objects does the coercion throw a catchable TypeError
//! (test262 trim 15.5.4.20-2-42 asserts both get called).
//!
//! Dispatch reuses [`crate::method_call::any_method_call_inner`] —
//! own/patched entries win, and the inherited Object.prototype
//! surface (`valueOf` → receiver, `toString` → "[object Object]")
//! provides the spec default when the receiver has neither.
//! RFC 20260712-string-proto-cluster chunk C.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, HeapHeader, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, box_pointer, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Method-dispatch indirection: the real
/// [`crate::method_call::any_method_call_inner`] reaches the whole
/// per-tag dispatch universe (dynobj / date / map / str-any
/// helpers), whose extern symbols only exist in the shipped
/// staticlib link. The unit-test binary must not pull that graph
/// past `-dead_strip` (coerce.rs is test-reachable), so tests get a
/// no-hook stub — the inherited-prototype default is exercised by
/// conformance, not unit tests.
#[cfg(not(test))]
#[inline]
unsafe fn dispatch_method(recv: AnyValue, mid: i64, name_str: *const u8) -> AnyValue {
    unsafe {
        crate::method_call::any_method_call_inner(
            recv,
            mid,
            name_str,
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
        )
    }
}

#[cfg(test)]
#[inline]
unsafe fn dispatch_method(_recv: AnyValue, _mid: i64, _name_str: *const u8) -> AnyValue {
    crate::nanbox::VALUE_UNDEFINED
}

/// §7.1.1.1 IsCallable probe — an own struct/dynobj entry holding a
/// non-callable value makes OrdinaryToPrimitive skip that method
/// name (`{toString: void 0, valueOf: fn}` coerces through valueOf;
/// pre-fix the dispatch's not-callable TypeError was mistaken for a
/// user throw and propagated — test262 search A1_T9).
#[cfg(not(test))]
unsafe fn skip_not_callable(cell: *mut c_void, name_str: *const u8) -> bool {
    let h = unsafe { &*(cell as *const HeapHeader) };
    match h.tag() {
        Tag::Obj => unsafe {
            crate::method_call_dynobj::own_entry_not_callable(cell, true, name_str)
        },
        Tag::DynObj => unsafe {
            crate::method_call_dynobj::own_entry_not_callable(cell, false, name_str)
        },
        _ => false,
    }
}

#[cfg(test)]
unsafe fn skip_not_callable(_cell: *mut c_void, _name_str: *const u8) -> bool {
    false
}

/// A NaN-boxed value is an "object" for ToPrimitive purposes iff it
/// is a heap cell whose tag is not `Str` (Str cells and ShortStr
/// immediates are the string primitive; every other immediate is a
/// primitive by construction).
#[inline]
fn is_object_value(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let h = unsafe { &*(as_void_ptr(v) as *const HeapHeader) };
    !matches!(h.tag(), Tag::Str)
}

/// `ToPrimitive(cell)` with the DEFAULT hint (§7.1.1 step 1.c):
/// every ordinary object treats default as number order, but a
/// Date's `@@toPrimitive` maps default to string order
/// (§21.4.4.45) — `date == str` compares the toString form, not
/// the epoch millis. Loose-equality's object arm (§7.2.14 steps
/// 11-12) is the consumer.
pub(crate) unsafe fn heap_to_primitive_default(cell: *mut c_void) -> Option<AnyValue> {
    let h = unsafe { &*(cell as *const HeapHeader) };
    let hint_string = matches!(h.tag(), Tag::Date);
    unsafe { heap_to_primitive(cell, hint_string) }
}

/// OrdinaryToPrimitive over a heap cell. `hint_string` picks the
/// method order (`toString` → `valueOf` for hint string, reversed
/// for hint number). Returns the first primitive result as a
/// caller-owned boxed value; when both methods answer objects,
/// records a catchable TypeError and returns `None`. A user method
/// that throws propagates immediately (pending-throw short
/// circuit) — the caller must answer a type-correct placeholder
/// and let its `emit_throw_check` unwind.
pub(crate) unsafe fn heap_to_primitive(cell: *mut c_void, hint_string: bool) -> Option<AnyValue> {
    let recv = box_pointer(cell as *mut HeapHeader);
    let order: [(i64, &[u8]); 2] = if hint_string {
        [
            (ANY_METHOD_TO_STRING, b"toString"),
            (ANY_METHOD_VALUE_OF, b"valueOf"),
        ]
    } else {
        [
            (ANY_METHOD_VALUE_OF, b"valueOf"),
            (ANY_METHOD_TO_STRING, b"toString"),
        ]
    };
    for (mid, name) in order {
        let key = unsafe { __torajs_str_alloc(name.as_ptr(), name.len() as i64) };
        if unsafe { skip_not_callable(cell, key) } {
            unsafe { __torajs_str_drop(key as *mut c_void) };
            continue;
        }
        let out = unsafe { dispatch_method(recv, mid, key) };
        unsafe { __torajs_str_drop(key as *mut c_void) };
        if unsafe { __torajs_throw_check() } != 0 {
            // The user method threw — propagate by returning a
            // primitive placeholder; the pending throw unwinds at
            // the caller's next check point.
            return Some(out);
        }
        if !is_object_value(out) {
            return Some(out);
        }
        unsafe { __torajs_anyv_rc_dec(out) };
    }
    unsafe {
        __torajs_throw_type_error(c"cannot convert object to primitive value".as_ptr());
    }
    None
}
