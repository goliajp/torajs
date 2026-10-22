//! Throw helpers for empty-array errors per ES spec.
//!
//! `[].reduce(fn)` and `[].reduceRight(fn)` with no `initialValue`
//! arg must throw `TypeError` per ES §22.1.3.21 step 3 / §22.1.3.22
//! step 3. The SSA arm emits an `i64 len == 0` guard before
//! consuming the seed element; on the throw branch it calls one of
//! these 0-arg helpers, which set the spec-correct message via
//! `__torajs_throw_type_error` and return — `emit_throw_check(None)`
//! at the call site propagates the pending throw to the nearest
//! catch or to `__torajs_uncaught_exit`.

use core::ffi::{c_char, c_void};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_throw_reduce_empty() {
    // SAFETY: NUL-terminated static C string.
    unsafe {
        __torajs_throw_type_error(c"reduce of empty array with no initial value".as_ptr());
    }
}

/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_throw_reduce_right_empty() {
    // SAFETY: NUL-terminated static C string.
    unsafe {
        __torajs_throw_type_error(c"reduceRight of empty array with no initial value".as_ptr());
    }
}

/// `reduce` / `reduceRight` without an initial value — §23.1.3.24 /
/// §23.1.3.25 step 8: the seed is the first PRESENT element in visiting
/// order (HasProperty, so a hole is skipped and an inherited index
/// counts). Answers its index; with none (empty, or all holes) it
/// records the TypeError and answers -1. The typed lowering and the
/// any-receiver kernel both start from here, so the two cannot differ
/// on which element seeds.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_reduce_seed_index(arr: *mut c_void, right: i64) -> i64 {
    unsafe {
        let len = *((arr as *const u8).add(crate::layout::ARR_LEN_OFF) as *const u64) as i64;
        let (mut k, step) = if right != 0 { (len - 1, -1) } else { (0, 1) };
        while k >= 0 && k < len {
            if crate::define_hole::__torajs_arr_has_index(arr, k) != 0 {
                return k;
            }
            k += step;
        }
        if right != 0 {
            __torajs_arr_throw_reduce_right_empty();
        } else {
            __torajs_arr_throw_reduce_empty();
        }
        -1
    }
}
