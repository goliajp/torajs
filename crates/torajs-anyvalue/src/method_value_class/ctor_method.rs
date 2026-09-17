//! What a CLASS-CONSTRUCTOR receiver answers for the borrowed
//! builtins — split from the parent so the K2b unit-face work had
//! room under the 500-line cap. The parent answers "how a class
//! MEMBER lands on a prototype as a callable face"; this answers
//! "what `C.toString()` / `C.valueOf()` / `C.bind(...)` do when `C`
//! itself is the receiver". Body verbatim; a child module reaches the
//! parent's private items directly — it happens to need none.

use core::ffi::c_void;

/// The borrowed-builtin answers a CLASS-CONSTRUCTOR dynobj receiver
/// owns (see the `generic_builtin_this` arm): `toString` /
/// `toLocaleString` answer the recorded class-declaration source
/// (§20.2.3.5 — an interned immortal Str, boxed as-is; `None` when
/// no source was recorded, e.g. lib/eval-injected classes), and
/// `valueOf` answers the receiver itself (§20.1.4.7 — inherited
/// Object.prototype identity, +1 per the boxed-value convention);
/// `bind` mints the bound cell over the class (`argv` / `argc` are
/// the call's own). `None` for every other receiver or mid.
///
/// # Safety
/// `recv` is a cell-encoded AnyValue over a live heap block.
pub(crate) unsafe fn class_ctor_method(
    mid: i64,
    recv: crate::nanbox::AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<crate::nanbox::AnyValue> {
    unsafe {
        let ptr = crate::nanbox::as_void_ptr(recv);
        if !crate::construct::is_class_ctor_cell(ptr) {
            return None;
        }
        if mid == torajs_rc::ANY_METHOD_VALUE_OF {
            torajs_rc::__torajs_rc_inc(ptr);
            return Some(recv);
        }
        // §20.2.3.2 Function.prototype.bind — the bound cell holds the
        // class object as its target (`method_bind`); `C.bind(null, 7)`
        // used to fall through to not_callable (RFC 20260915).
        if mid == torajs_rc::ANY_METHOD_BIND {
            return Some(crate::method_bind::bind_cell(ptr, argv, argc));
        }
        if !matches!(
            mid,
            torajs_rc::ANY_METHOD_TO_STRING | torajs_rc::ANY_METHOD_TO_LOCALE_STRING
        ) {
            return None;
        }
        let src = class_source_lookup(ptr);
        if src.is_null() {
            return None;
        }
        Some(crate::nanbox_encode::__torajs_anyv_box_pointer(src))
    }
}

/// torajs-meta's per-tag class source table. The unit-test binary
/// links without the staticlib graph (same posture as
/// `to_primitive::dispatch_method`'s test stub), so tests see a
/// permanent miss — the class-source behavior is exercised by
/// conformance.
#[cfg(not(test))]
#[inline]
unsafe fn class_source_lookup(cell: *const c_void) -> *mut c_void {
    unsafe extern "C" {
        fn __torajs_class_source_for_cell(cell: *const c_void) -> *mut c_void;
    }
    unsafe { __torajs_class_source_for_cell(cell) }
}

#[cfg(test)]
#[inline]
unsafe fn class_source_lookup(_cell: *const c_void) -> *mut c_void {
    core::ptr::null_mut()
}
