//! Error-family class capture — feeds the torajs-rc
//! `native_error_class` registry the globalThis fill
//! (torajs-anyvalue `globalthis_object.rs`) reads at mint time, so
//! the dynamic `globalThis.TypeError` read answers the SAME class
//! object the bare name resolves to.
//!
//! Runs inside `__torajs_error_proto_install`, whose emit follows
//! every `__torajs_class_register` (class_globals_register.rs emit
//! order is load-bearing), so `CLASSES_BY_TAG_IMM[tag]` is already
//! filled when the capture reads it. Only injected error classes
//! emit the install, so a non-family name never reaches the match.

use core::ffi::c_void;

/// Mirror of `torajs_rc::builtin_proto::native_error_class::
/// NATIVE_ERROR_FAMILY` (append-only ABI index order; same mirror
/// discipline as `ANY_METHOD_ERROR_TO_STRING_MID` next door — meta
/// keeps a zero-Cargo-dep tree and reaches the store through the
/// C symbol).
const NATIVE_ERROR_FAMILY: [&[u8]; 9] = [
    b"Error",
    b"TypeError",
    b"RangeError",
    b"ReferenceError",
    b"SyntaxError",
    b"EvalError",
    b"URIError",
    b"AggregateError",
    b"SuppressedError",
];

unsafe extern "C" {
    fn __torajs_native_error_class_record(idx: i64, class_anyv: u64);
}

/// Record `CLASSES_BY_TAG_IMM[tag]` under `name`'s family index.
/// Non-cell slots (test sentinels) and non-family names are ignored.
///
/// # Safety
/// `name` is a live Str cell; `tag` passed the caller's `in_range`.
pub(super) unsafe fn record_error_family_class(tag: i64, name: *const c_void) {
    let Some(idx) = NATIVE_ERROR_FAMILY
        .iter()
        .position(|fam| super::str_is(name, fam))
    else {
        return;
    };
    // SAFETY: single-threaded module init; slot was written by the
    // class_register emit that precedes this install.
    let class_anyv = unsafe { super::CLASSES_BY_TAG_IMM[tag as usize] };
    if super::is_cell_imm(class_anyv) {
        unsafe { __torajs_native_error_class_record(idx as i64, class_anyv) };
    }
}
