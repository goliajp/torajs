//! Cross-crate shells over the class-layout DATA-field store and
//! compare — RFC 20260806-declared-field-redefine.
//!
//! `Object.defineProperty` on a class instance is implemented in
//! torajs-dynobj (it owns the `+24` expando dict where the redefined
//! field's ATTRIBUTES live), but the field's VALUE lives in the layout
//! slot, whose typed store rules — slot-type match, frozen gate, rc
//! transfer — are already settled here by the `a.x = v` path. Rather
//! than restate those rules across the crate line, the define arm
//! calls back through these two symbols.
//!
//! Both are thin: no logic beyond the raw↔bool boundary conversion.

use core::ffi::c_void;

/// Write `[[Value]]` into the layout slot the DECLARED field `key`
/// names. Answers 1 when the store landed — or when the frozen gate
/// refused it LOUDLY, having recorded its own TypeError, since either
/// way the caller must not go on to write attributes. Answers 0 when
/// `key` names no data field of `ptr`'s layout, or when the payload
/// does not fit the slot's type (a typed slot never silently
/// coerces).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell. The caller transfers one rc of a `Heap`-tagged `value`,
/// consumed by the store.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_data_field_set(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> i64 {
    i64::from(unsafe { crate::struct_error_msg::struct_data_field_set(ptr, key, tag, value) })
}

/// Does the DECLARED field `key` already hold `(tag, value)`?
///
/// §10.1.6.3 step 4 lets a redefine through on a non-writable,
/// non-configurable property when the incoming `[[Value]]` is
/// SameValue with the current one — so the define arm has to be able
/// to ask, and only this crate can decode a typed slot into a
/// comparable pair. Equality is the exact `(tag, value)` match used
/// everywhere else `Any === Any` is approximated.
///
/// Answers 0 for a missing layout / absent field, which is the honest
/// reading: nothing is stored there to match.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell. Both operands are BORROWED — no stake is taken or released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_data_field_same_value(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> i64 {
    match unsafe { crate::struct_probe::struct_field_pair(ptr, key) } {
        Some((cur_tag, cur_value)) => i64::from(cur_tag == tag && cur_value == value),
        None => 0,
    }
}
