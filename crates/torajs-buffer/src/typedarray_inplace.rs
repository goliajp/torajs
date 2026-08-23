//! `at` / `fill` / `copyWithin` / `reverse`
//! (RFC 20260823-typedarray-substrate 刀 5, slab A first half).
//!
//! These four share a shape: nothing here allocates, and everything
//! here has to survive its own argument coercions. §23.2.3.9 (fill)
//! is the model the two mutators follow exactly —
//!
//! 1. validate and take a length,
//! 2. coerce EVERY argument against that length,
//! 3. throw the extent away and ask for it again,
//! 4. re-clamp the already-computed indices to the new length.
//!
//! Step 3 looks redundant and is not: a `valueOf` in step 2 is user
//! code and can detach the buffer or shrink a resizable one, so the
//! clamps from step 2 were measured against a length that no longer
//! exists. Writing through the step-1 base pointer after that is the
//! use-after-free this ordering exists to prevent.
//!
//! An absent argument and an explicit `undefined` are the same thing
//! for every one of them (`end` defaults to the length either way),
//! which is why the ABI takes plain slots and the dispatcher fills
//! the missing ones with `undefined`.

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED};

use crate::typedarray_elem;
use crate::typedarray_span::{clamp_relative, revalidate, to_integer_or_infinity, validate};

unsafe extern "C" {
    /// §10.4.5.4 [[Get]] — the same entry the `ta[i]` lowering uses,
    /// so `at` cannot drift from it.
    fn __torajs_typedarray_index_get(av: AnyValue, index: f64) -> AnyValue;
    fn __torajs_anyv_rc_inc(v: AnyValue);
}

/// The three mutators answer the receiver (§23.2.3.9 step 12,
/// §23.2.3.6 step 16, §23.2.3.24 step 5), and a returned value is
/// OWNED on this ABI — the array mutators next door take the same
/// fresh stake for the same reason. Handing the borrow straight back
/// would leave the caller dropping a reference it never held.
///
/// # Safety
/// `recv` is a live AnyValue.
#[inline]
unsafe fn owned(recv: AnyValue) -> AnyValue {
    unsafe { __torajs_anyv_rc_inc(recv) };
    recv
}

/// §23.2.3.1 `%TypedArray%.prototype.at`. Two lengths are in play
/// and the spec is deliberate about which is which: step 6's bounds
/// test uses the length taken BEFORE the index was coerced, while
/// step 7 is an ordinary §10.4.5 [[Get]] that re-derives the extent
/// itself. So an index that was in range when it was computed, over
/// a buffer the coercion then detached, answers `undefined` — it
/// does not throw, and it does not read freed bytes.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_at(recv: AnyValue, index: AnyValue) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let len = span.len;
        let Some(rel) = to_integer_or_infinity(index) else {
            return VALUE_UNDEFINED;
        };
        let k = if rel >= 0.0 { rel } else { len as f64 + rel };
        if k < 0.0 || k >= len as f64 {
            return VALUE_UNDEFINED;
        }
        __torajs_typedarray_index_get(recv, k)
    }
}

/// §23.2.3.9 `%TypedArray%.prototype.fill`.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_fill(
    recv: AnyValue,
    value: AnyValue,
    start: AnyValue,
    end: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let len = span.len;
        // Steps 4-5 — the element type decides which coercion runs,
        // and the two are not interchangeable: a BigInt array
        // rejects a Number rather than converting it.
        let Some(coerced) = typedarray_elem::coerce(span.kind, value) else {
            return VALUE_UNDEFINED;
        };
        let Some(rel_start) = to_integer_or_infinity(start) else {
            return VALUE_UNDEFINED;
        };
        let mut k = clamp_relative(rel_start, len);
        let mut final_ = len;
        if end != VALUE_UNDEFINED {
            let Some(rel_end) = to_integer_or_infinity(end) else {
                return VALUE_UNDEFINED;
            };
            final_ = clamp_relative(rel_end, len);
        }
        // Step 10 — the coercions above were user code. Ask again.
        let Some(span) = revalidate(recv) else {
            return VALUE_UNDEFINED;
        };
        if final_ > span.len {
            final_ = span.len;
        }
        if k > span.len {
            k = span.len;
        }
        while k < final_ {
            typedarray_elem::store(span.base, span.kind, k, coerced);
            k += 1;
        }
        owned(recv)
    }
}

/// §23.2.3.6 `%TypedArray%.prototype.copyWithin` — a `memmove` over
/// the element bytes, so an overlapping range copies correctly and
/// the element type is never looked at.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_copy_within(
    recv: AnyValue,
    target: AnyValue,
    start: AnyValue,
    end: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let len = span.len;
        let Some(rel_to) = to_integer_or_infinity(target) else {
            return VALUE_UNDEFINED;
        };
        let mut to = clamp_relative(rel_to, len);
        let Some(rel_from) = to_integer_or_infinity(start) else {
            return VALUE_UNDEFINED;
        };
        let mut from = clamp_relative(rel_from, len);
        let mut final_ = len;
        if end != VALUE_UNDEFINED {
            let Some(rel_end) = to_integer_or_infinity(end) else {
                return VALUE_UNDEFINED;
            };
            final_ = clamp_relative(rel_end, len);
        }
        let mut count = (final_ - from).min(len - to);
        if count <= 0 {
            return owned(recv);
        }
        // Step 14 — same re-ask as fill, and the same reason.
        let Some(span) = revalidate(recv) else {
            return VALUE_UNDEFINED;
        };
        if to > span.len {
            to = span.len;
        }
        if from > span.len {
            from = span.len;
        }
        if to + count > span.len {
            count = span.len - to;
        }
        if from + count > span.len {
            count = span.len - from;
        }
        if count <= 0 {
            return owned(recv);
        }
        let esize = span.kind.element_size() as usize;
        core::ptr::copy(
            span.base.add(from as usize * esize),
            span.base.add(to as usize * esize),
            count as usize * esize,
        );
        owned(recv)
    }
}

/// §23.2.3.24 `%TypedArray%.prototype.reverse` — in place, and the
/// only member of the slab that runs no user code at all, so the
/// extent it validated is still the extent it writes through.
///
/// # Safety
/// `recv` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_reverse(recv: AnyValue) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let esize = span.kind.element_size() as usize;
        let mut lo = 0i64;
        let mut hi = span.len - 1;
        while lo < hi {
            core::ptr::swap_nonoverlapping(
                span.base.add(lo as usize * esize),
                span.base.add(hi as usize * esize),
                esize,
            );
            lo += 1;
            hi -= 1;
        }
        owned(recv)
    }
}
