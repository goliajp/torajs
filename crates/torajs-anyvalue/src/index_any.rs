//! `__torajs_any_index_get` — `recv[idx]` where the receiver is an
//! `any` value (Any-dynamic-access RFC 20260704 S3).
//!
//! Dispatch tree (ES ordinary property access semantics):
//! - `null` / `undefined` receiver → catchable TypeError (ES §13.3.2
//!   RequireObjectCoercible), returns `undefined` after recording the
//!   pending throw (caller's `emit_throw_check` propagates).
//! - numeric / bool immediates → `undefined` (primitives have no
//!   own indexed properties).
//! - ShortStr — ASCII payloads answer inline (byte == code unit);
//!   non-ASCII payloads materialize to a heap Str and reuse the heap
//!   path so UTF-16 code-unit semantics match the typed tier.
//! - `Tag::Str` cell (Str or Substr view) →
//!   `__torajs_str_index_get` (torajs-str); NULL = OOB → `undefined`.
//! - `Tag::Arr` cell → `__torajs_arr_index_get` (torajs-arr,
//!   kind-aware: FLAG_ARR_ANY NaN-box slots or `ARR_KIND_*` raw
//!   slots recorded at the boxing boundary).
//! - `Tag::DynObj` → explicit TypeError — numeric-key property
//!   lookup on plain objects is the RFC's S4 follow-up (a roadmap
//!   boundary, never a silent wrong answer).
//! - any other heap tag → `undefined` (no such own property).

use core::ffi::c_void;

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len, try_box_short_str,
};
use crate::nanbox_ffi_materialize::materialize_short_str;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-str — `s[idx]` (Str or Substr); NULL = OOB.
    fn __torajs_str_index_get(s: *mut u8, idx: i64) -> *mut u8;
    /// torajs-arr — kind-aware `arr[idx]`; returns a balanced
    /// AnyValue (+1 for cells).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-str — release a heap Str/Substr reference. Signature
    /// mirrors the crate-local test stub (`*mut c_void`).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_get(recv: AnyValue, idx: i64) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            // ASCII: byte index == UTF-16 code-unit index.
            if idx < 0 || idx as usize >= len {
                return VALUE_UNDEFINED;
            }
            return try_box_short_str(&payload[idx as usize..idx as usize + 1])
                .unwrap_or(VALUE_UNDEFINED);
        }
        // Non-ASCII payload: materialize to a heap Str and reuse the
        // heap path so code-unit semantics match the typed tier. The
        // returned Substr holds its own parent reference, so the
        // temporary parent drops safely right after.
        unsafe {
            let parent = materialize_short_str(recv);
            let out = index_str_cell(parent, idx);
            __torajs_str_drop(parent as *mut c_void);
            return out;
        }
    }
    if is_int32(recv) || is_double(recv) || is_bool(recv) {
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    if tag == Tag::Str as u16 {
        return unsafe { index_str_cell(ptr as *mut u8, idx) };
    }
    if tag == Tag::Arr as u16 {
        return unsafe { __torajs_arr_index_get(ptr, idx) };
    }
    if tag == Tag::DynObj as u16 {
        unsafe {
            __torajs_throw_type_error(
                c"indexing a plain object through any is not yet implemented".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    VALUE_UNDEFINED
}

/// Shared `Tag::Str` arm — `str_index_get` returns a fresh rc=1
/// Substr (ownership transfers into the box) or NULL for OOB.
unsafe fn index_str_cell(s: *mut u8, idx: i64) -> AnyValue {
    unsafe {
        let sub = __torajs_str_index_get(s, idx);
        if sub.is_null() {
            VALUE_UNDEFINED
        } else {
            box_void_ptr(sub as *mut c_void)
        }
    }
}
