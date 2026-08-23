//! `subarray` / `slice` / `toReversed` / `with`
//! (RFC 20260823-typedarray-substrate 刀 5, slab A copy half).
//!
//! The four that answer a NEW object, and the line between them is
//! whether the bytes are shared or copied: `subarray` mints another
//! view over the SAME buffer, so a write through either is visible
//! through the other; the other three allocate.
//!
//! `subarray` is also the one member of the slab that does not
//! validate. §23.2.3.28 asks only for the internal slot and then
//! takes the source length as 0 when the view is out of bounds — so
//! `detached.subarray(0)` answers an empty view where
//! `detached.slice(0)` throws. Two neighbouring methods, two
//! different answers to the same state, and the difference is
//! written into the step lists rather than derived from anything.
//!
//! None of them consults `@@species`; that is still out (see the
//! RFC), and every "create" here is TypedArrayCreateSameType.

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};

use crate::typedarray::{
    AUTO_LENGTH, array_len_of, byte_offset_of, is_typedarray, kind_of, resolve,
};
use crate::typedarray_ctor::{create_same_type, mint};
use crate::typedarray_elem;
use crate::typedarray_span::{clamp_relative, revalidate, to_integer_or_infinity, validate};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_rc_dec(v: AnyValue);
}

/// §23.2.3.28 `%TypedArray%.prototype.subarray` — a second view over
/// the same bytes.
///
/// Step 12 is the subtle one: a length-tracking source with no `end`
/// argument produces a length-TRACKING result, not one pinned to the
/// length it happens to have now. Pinning it would make the new view
/// stop growing with its buffer, which is a different object.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_subarray(
    recv: AnyValue,
    start: AnyValue,
    end: AnyValue,
) -> AnyValue {
    unsafe {
        if !is_typedarray(recv) {
            __torajs_throw_type_error(
                b"this is not a typed array\0".as_ptr() as *const core::ffi::c_char
            );
            return VALUE_UNDEFINED;
        }
        let ptr = as_void_ptr(recv);
        let kind = kind_of(ptr);
        // Steps 4-6 — out of bounds is a length of 0 here, not a throw.
        let src_len = resolve(ptr).map_or(0, |(_, l)| l);
        let Some(rel_start) = to_integer_or_infinity(start) else {
            return VALUE_UNDEFINED;
        };
        let start_index = clamp_relative(rel_start, src_len);
        let mut end_index = src_len;
        if end != VALUE_UNDEFINED {
            let Some(rel_end) = to_integer_or_infinity(end) else {
                return VALUE_UNDEFINED;
            };
            end_index = clamp_relative(rel_end, src_len);
        }
        let esize = kind.element_size();
        let begin = byte_offset_of(ptr) + start_index * esize;
        let buffer = crate::typedarray::buffer_of(ptr);
        // Step 12 — tracking stays tracking.
        let new_len = if array_len_of(ptr) == AUTO_LENGTH && end == VALUE_UNDEFINED {
            AUTO_LENGTH
        } else {
            (end_index - start_index).max(0)
        };
        // Step 15 TypedArraySpeciesCreate — constructor-face read
        // (`species.rs`), ahead of the mint.
        if crate::species::__torajs_buffer_species_guard(recv) != 0 {
            return VALUE_UNDEFINED;
        }
        mint(kind, buffer, begin, new_len)
    }
}

/// §23.2.3.27 `%TypedArray%.prototype.slice` — a copy, and unlike
/// `subarray` it validates and can throw.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_slice(
    recv: AnyValue,
    start: AnyValue,
    end: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let len = span.len;
        let kind = span.kind;
        let Some(rel_start) = to_integer_or_infinity(start) else {
            return VALUE_UNDEFINED;
        };
        let k = clamp_relative(rel_start, len);
        let mut final_ = len;
        if end != VALUE_UNDEFINED {
            let Some(rel_end) = to_integer_or_infinity(end) else {
                return VALUE_UNDEFINED;
            };
            final_ = clamp_relative(rel_end, len);
        }
        let count = (final_ - k).max(0);
        // Step 9 TypedArraySpeciesCreate begins with the
        // constructor-face read (`species.rs`) — an instance-
        // installed throwing getter surfaces here.
        if crate::species::__torajs_buffer_species_guard(recv) != 0 {
            return VALUE_UNDEFINED;
        }
        let out = create_same_type(kind, count);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if count == 0 {
            return out;
        }
        // Step 8 — the coercions were user code, so the source is
        // re-derived and the copy is clipped to what is still there.
        let Some(span) = revalidate(recv) else {
            __torajs_anyv_rc_dec(out);
            return VALUE_UNDEFINED;
        };
        let take = (final_.min(span.len) - k).max(0);
        if take > 0 {
            let esize = kind.element_size() as usize;
            let Some((dst, _)) = resolve(as_void_ptr(out)) else {
                return out;
            };
            core::ptr::copy_nonoverlapping(
                span.base.add(k as usize * esize),
                dst,
                take as usize * esize,
            );
        }
        out
    }
}

/// §23.2.3.32 `%TypedArray%.prototype.toReversed` — a reversed copy.
/// No argument, so no user code, so the extent it validated is the
/// extent it reads.
///
/// # Safety
/// `recv` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_to_reversed(recv: AnyValue) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let out = create_same_type(span.kind, span.len);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let Some((dst, _)) = resolve(as_void_ptr(out)) else {
            return out;
        };
        let esize = span.kind.element_size() as usize;
        for k in 0..span.len {
            core::ptr::copy_nonoverlapping(
                span.base.add((span.len - 1 - k) as usize * esize),
                dst.add(k as usize * esize),
                esize,
            );
        }
        out
    }
}

/// §23.2.3.36 `%TypedArray%.prototype.with` — a copy with one
/// element replaced.
///
/// Step 7 is the one worth naming: the index is validated AFTER the
/// value has been coerced, and against the extent as it is by then.
/// So a `valueOf` that shrinks the buffer turns an index that was in
/// range into a RangeError, and the RangeError is raised before
/// anything is allocated.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_with(
    recv: AnyValue,
    index: AnyValue,
    value: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_UNDEFINED;
        };
        let len = span.len;
        let kind = span.kind;
        let Some(rel) = to_integer_or_infinity(index) else {
            return VALUE_UNDEFINED;
        };
        let actual = if rel >= 0.0 { rel } else { len as f64 + rel };
        // Step 6 — the element type picks the coercion, and it runs
        // whether or not the index turns out to be valid.
        let Some(coerced) = typedarray_elem::coerce(kind, value) else {
            return VALUE_UNDEFINED;
        };
        let Some(span) = revalidate(recv) else {
            return VALUE_UNDEFINED;
        };
        if actual < 0.0 || actual >= span.len as f64 || actual.trunc() != actual {
            __torajs_throw_range_error(b"Invalid typed array index\0".as_ptr());
            return VALUE_UNDEFINED;
        }
        let actual = actual as i64;
        let out = create_same_type(kind, len);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let Some((dst, _)) = resolve(as_void_ptr(out)) else {
            return out;
        };
        let esize = kind.element_size() as usize;
        // Step 9 walks the PRE-coercion length. An index past what
        // is live now reads `undefined` there, and the store coerces
        // it — NaN for a float kind, 0 for an integer one. A BigInt
        // kind cannot: ToBigInt(undefined) is a TypeError, which is
        // what falls out of `coerce` below.
        let live = span.len;
        for k in 0..len {
            if k == actual {
                typedarray_elem::store(dst, kind, k, coerced);
            } else if k < live {
                core::ptr::copy_nonoverlapping(
                    span.base.add(k as usize * esize),
                    dst.add(k as usize * esize),
                    esize,
                );
            } else {
                let Some(c) = typedarray_elem::coerce(kind, VALUE_UNDEFINED) else {
                    __torajs_anyv_rc_dec(out);
                    return VALUE_UNDEFINED;
                };
                typedarray_elem::store(dst, kind, k, c);
            }
        }
        out
    }
}
