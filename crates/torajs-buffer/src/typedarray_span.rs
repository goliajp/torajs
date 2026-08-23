//! The spine every `%TypedArray%.prototype` method starts from
//! (RFC 20260823-typedarray-substrate 刀 5).
//!
//! Two things are shared by the whole slab and are worth having in
//! one place, because getting either subtly wrong is invisible on
//! ordinary input:
//!
//! - **§23.2.4.4 ValidateTypedArray throws.** Indexed access
//!   (§10.4.5) answers `undefined` for a detached or out-of-bounds
//!   view; a *method* on the same view raises a TypeError. Same
//!   object, same state, two different answers — so the method entry
//!   cannot reuse the index path's silent miss.
//!
//! - **Every argument coercion can invalidate the extent.** A
//!   `valueOf` is user code and may detach the buffer or resize it.
//!   The spec therefore coerces every argument first, and only then
//!   re-derives the length and re-checks the bounds (§23.2.3.9
//!   step 10 is the clearest statement of it). Nothing here holds a
//!   length across a coercion; callers ask [`validate`] again.

use torajs_anyvalue::nanbox::{AnyValue, as_void_ptr};

use crate::typedarray::{Kind, is_typedarray, kind_of, resolve};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
}

/// What a view is right now, for a method that is allowed to throw.
pub(crate) struct Span {
    pub base: *mut u8,
    pub len: i64,
    pub kind: Kind,
}

/// §23.2.4.4 ValidateTypedArray — the brand check plus the extent,
/// raising a TypeError rather than answering a miss.
///
/// `None` means the pending throw is recorded and the caller returns
/// its own undefined immediately.
///
/// # Safety
/// `av` is a live AnyValue.
pub(crate) unsafe fn validate(av: AnyValue) -> Option<Span> {
    if !is_typedarray(av) {
        unsafe {
            __torajs_throw_type_error(
                b"this is not a typed array\0".as_ptr() as *const core::ffi::c_char
            )
        };
        return None;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        let Some((base, len)) = resolve(ptr) else {
            __torajs_throw_type_error(
                b"the typed array's buffer is detached or out of bounds\0".as_ptr()
                    as *const core::ffi::c_char,
            );
            return None;
        };
        Some(Span {
            base,
            len,
            kind: kind_of(ptr),
        })
    }
}

/// Re-derive the extent after user code has had a chance to run,
/// with §23.2.3.9-step-10's answer for a view that no longer fits:
/// a TypeError, not a clamp to nothing.
///
/// # Safety
/// `av` is a live TypedArray AnyValue.
pub(crate) unsafe fn revalidate(av: AnyValue) -> Option<Span> {
    unsafe {
        let ptr = as_void_ptr(av);
        let Some((base, len)) = resolve(ptr) else {
            __torajs_throw_type_error(
                b"the typed array's buffer is detached or out of bounds\0".as_ptr()
                    as *const core::ffi::c_char,
            );
            return None;
        };
        Some(Span {
            base,
            len,
            kind: kind_of(ptr),
        })
    }
}

/// §23.2.4.4 ValidateTypedArray at the ABI, for the callers that
/// live outside this crate: the length, or -1 with a pending throw.
///
/// Sorting is the caller that needs it. §23.2.3.29 reads every
/// element into a List, orders the List, and writes it back — which
/// is an AnyValue-level walk over a user comparator, so it belongs
/// on the any-lane side. Only the brand-and-extent question has to
/// come from here.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_validate(av: AnyValue) -> i64 {
    match unsafe { validate(av) } {
        Some(span) => span.len,
        None => -1,
    }
}

/// §23.2.4.2 TypedArrayCreateSameType at the ABI — a fresh view of
/// the receiver's element type over its own buffer.
///
/// # Safety
/// `av` is a live TypedArray AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_create_same_type(av: AnyValue, len: i64) -> AnyValue {
    if !is_typedarray(av) {
        return torajs_anyvalue::nanbox::VALUE_UNDEFINED;
    }
    unsafe { crate::typedarray_ctor::create_same_type(kind_of(as_void_ptr(av)), len) }
}

/// §7.1.5 ToIntegerOrInfinity, kept as an `f64` so that the two
/// infinities survive to the clamp — `Infinity` and `len` are the
/// same clamped index but not the same number, and a saturating cast
/// to `i64` here would silently make them the same thing on the way
/// in as well.
///
/// `None` means the coercion threw.
///
/// # Safety
/// `v` is a live AnyValue.
pub(crate) unsafe fn to_integer_or_infinity(v: AnyValue) -> Option<f64> {
    let n = unsafe { __torajs_anyv_to_number(v) };
    if unsafe { __torajs_throw_check() } != 0 {
        return None;
    }
    if n.is_nan() {
        return Some(0.0);
    }
    if n.is_infinite() {
        return Some(n);
    }
    Some(n.trunc())
}

/// The relative-index clamp §23.2.3 spells out once per method:
/// negative counts back from the end, and the result is pinned to
/// `[0, len]`. `Infinity` lands on `len` and `-Infinity` on 0 by the
/// same arithmetic, which is why the input is still an `f64`.
pub(crate) fn clamp_relative(rel: f64, len: i64) -> i64 {
    if rel < 0.0 {
        let k = len as f64 + rel;
        if k < 0.0 { 0 } else { k as i64 }
    } else if rel > len as f64 {
        len
    } else {
        rel as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_clamp_matches_the_spec_table() {
        assert_eq!(clamp_relative(0.0, 5), 0);
        assert_eq!(clamp_relative(2.0, 5), 2);
        assert_eq!(clamp_relative(9.0, 5), 5);
        assert_eq!(clamp_relative(-1.0, 5), 4);
        assert_eq!(clamp_relative(-9.0, 5), 0);
        assert_eq!(clamp_relative(f64::INFINITY, 5), 5);
        assert_eq!(clamp_relative(f64::NEG_INFINITY, 5), 0);
        // An empty view pins everything to 0 rather than underflowing.
        assert_eq!(clamp_relative(-3.0, 0), 0);
        assert_eq!(clamp_relative(3.0, 0), 0);
    }
}
