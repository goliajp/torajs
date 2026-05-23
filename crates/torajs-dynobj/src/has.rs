//! DynObj key-presence check.
//!
//! Port of `runtime_str.c::__torajs_dynobj_has` (P4.2-e, 2026-05-23).
//! Pure probe lookup — discards the bucket index, returns 1 / 0.
//! Spec §10.1.7 OrdinaryHasOwnProperty data-property case.

use core::ffi::c_void;

use crate::probe::probe;

/// `__torajs_dynobj_has(obj, key)` — return 1 iff `key` is present in
/// `obj`, else 0. NULL `obj` is treated as "not present".
///
/// # Safety
/// `obj` is null or a live dynobj heap pointer. `key` (if reached)
/// is a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32 {
    if obj.is_null() {
        return 0;
    }
    let pr = unsafe { probe(obj, key) };
    if pr.found { 1 } else { 0 }
}
