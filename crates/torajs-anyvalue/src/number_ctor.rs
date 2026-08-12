//! §21.1.1.1 — the explicit `Number(value)` call's coercion face for
//! an `any`-typed argument. `Number(x)` runs ToNumeric: a BigInt
//! primitive legally converts (𝔽(ℝ(value)), the torajs-bigint
//! kernel), while every other shape answers exactly what the generic
//! §7.1.4 ToNumber kernel already answers (Symbol / mixed-BigInt
//! arithmetic keep THROWING there — this pre-gate is the one legal
//! BigInt window). Same pre-gate-then-delegate shape as
//! [`crate::nanbox_ffi::__torajs_anyv_to_display_str`], String()'s
//! Symbol window.

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_bigint_to_number(b: *const u8) -> f64;
}

/// `Number(any)` — BigInt cells convert, everything else delegates.
///
/// # Safety
/// Cell case: the encoded pointer must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_number_ctor(v: AnyValue) -> f64 {
    if is_cell(v) {
        let ptr = as_void_ptr(v);
        // SAFETY: is_cell guarantees a live heap header.
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::BigInt as u16 {
            // SAFETY: tag says BigInt; the kernel borrows.
            return unsafe { __torajs_bigint_to_number(ptr as *const u8) };
        }
    }
    // SAFETY: same contract as this fn.
    unsafe { crate::nanbox_ffi::__torajs_anyv_to_number(v) }
}
