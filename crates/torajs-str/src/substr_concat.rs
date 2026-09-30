//! Substr-involved concat FFI shims — three permutations of
//! `(Substr + Str)` / `(Str + Substr)` / `(Substr + Substr)`, all
//! routed through [`crate::join`] for encoding-aware single-
//! alloc concatenation.

use core::ffi::c_void;

use crate::join::{Side, join};

/// `(substr + str)` — single-alloc view-aware concat.
///
/// P11.1-S2.5 Round 2 — encoding-aware via [`crate::join`].
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_str(
    v: *const u8,
    s: *const u8,
) -> *mut c_void {
    unsafe { join(Side::of_view(v), Side::of_str(s)) as *mut c_void }
}

/// `(str + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `s` is a live `*const Str`, `v` is a live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_str_substr(
    s: *const u8,
    v: *const u8,
) -> *mut c_void {
    unsafe { join(Side::of_str(s), Side::of_view(v)) as *mut c_void }
}

/// `(substr + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `a` and `b` are live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_substr(
    a: *const u8,
    b: *const u8,
) -> *mut c_void {
    unsafe { join(Side::of_view(a), Side::of_view(b)) as *mut c_void }
}
