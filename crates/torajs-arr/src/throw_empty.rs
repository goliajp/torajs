//! Throw helpers for empty-array errors per ES spec.
//!
//! `[].reduce(fn)` and `[].reduceRight(fn)` with no `initialValue`
//! arg must throw `TypeError` per ES §22.1.3.21 step 3 / §22.1.3.22
//! step 3. The SSA arm emits an `i64 len == 0` guard before
//! consuming the seed element; on the throw branch it calls one of
//! these 0-arg helpers, which set the spec-correct message via
//! `__torajs_throw_type_error` and return — `emit_throw_check(None)`
//! at the call site propagates the pending throw to the nearest
//! catch or to `__torajs_uncaught_exit_code`.

use core::ffi::c_char;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_throw_reduce_empty() {
    // SAFETY: NUL-terminated static C string.
    unsafe {
        __torajs_throw_type_error(c"reduce of empty array with no initial value".as_ptr());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_throw_reduce_right_empty() {
    // SAFETY: NUL-terminated static C string.
    unsafe {
        __torajs_throw_type_error(c"reduceRight of empty array with no initial value".as_ptr());
    }
}
