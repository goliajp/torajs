//! §23.2.3.26 `%TypedArray%.prototype.set`
//! (RFC 20260823-typedarray-substrate 刀 5, slab A last member).
//!
//! One name, two operations the spec keeps deliberately apart.
//!
//! §23.2.3.26.1 takes another typed array and copies element by
//! element with conversion; §23.2.3.26.2 takes anything else, reads
//! its `length` once and then each index in turn. The second one
//! NEVER consults `@@iterator` — which is why it cannot borrow the
//! constructor's `from_list`, whose whole job is to make that test.
//! `new Uint8Array(set)` reads three elements out of a `Set`;
//! `ta.set(set)` reads `length`, finds undefined, and stores
//! nothing. Same object, two answers, and sharing the walk would
//! quietly give it one.
//!
//! `set` also does not ValidateTypedArray on entry: §23.2.3.26 step
//! 2 is RequireInternalSlot, and the extent check happens inside
//! each of the two operations, AFTER `offset` has been coerced.
//! Coercing first is observable — a `valueOf` on the offset of a
//! detached target runs before the TypeError.

use core::ffi::c_char;

use torajs_anyvalue::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_void_ptr};

use crate::typedarray::{Kind, buffer_of, is_typedarray, kind_of, resolve};
use crate::typedarray_elem::{self, Coerced};
use crate::typedarray_span::to_integer_or_infinity;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_rc_dec(v: AnyValue);
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    /// §7.3.2 Get with an integer key, over any receiver.
    fn __torajs_any_index_get(recv: AnyValue, idx: i64) -> AnyValue;
    /// `Get(obj, "length")` — the array-like half of
    /// §7.3.18 LengthOfArrayLike.
    fn __torajs_any_length_get(recv: AnyValue) -> AnyValue;
}

/// §23.2.3.26.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_set(
    recv: AnyValue,
    source: AnyValue,
    offset: AnyValue,
) -> AnyValue {
    unsafe {
        if !is_typedarray(recv) {
            __torajs_throw_type_error(c"this is not a typed array".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Step 3 runs before any extent is looked at, and it is user
        // code.
        let Some(off) = to_integer_or_infinity(offset) else {
            return VALUE_UNDEFINED;
        };
        if off < 0.0 {
            __torajs_throw_range_error(b"offset is out of bounds\0".as_ptr());
            return VALUE_UNDEFINED;
        }
        if is_typedarray(source) {
            set_from_typedarray(recv, off, source);
        } else {
            set_from_array_like(recv, off, source);
        }
        VALUE_UNDEFINED
    }
}

/// The target's extent, or a recorded TypeError.
///
/// # Safety
/// `target` is a live TypedArray AnyValue.
unsafe fn target_span(target: AnyValue) -> Option<(*mut u8, i64, Kind)> {
    unsafe {
        let ptr = as_void_ptr(target);
        match resolve(ptr) {
            Some((base, len)) => Some((base, len, kind_of(ptr))),
            None => {
                __torajs_throw_type_error(
                    c"the typed array's buffer is detached or out of bounds".as_ptr(),
                );
                None
            }
        }
    }
}

/// Steps 11-12, shared: an infinite offset and a source that does
/// not fit are both RangeErrors, and both are checked before a
/// single element moves.
unsafe fn fits(off: f64, src_len: i64, target_len: i64) -> Option<i64> {
    if off.is_infinite() || src_len as f64 + off > target_len as f64 {
        unsafe { __torajs_throw_range_error(b"offset is out of bounds\0".as_ptr()) };
        return None;
    }
    Some(off as i64)
}

/// §23.2.3.26.1 SetTypedArrayFromTypedArray.
///
/// # Safety
/// Both are live TypedArray AnyValues.
unsafe fn set_from_typedarray(target: AnyValue, off: f64, source: AnyValue) {
    unsafe {
        let Some((dbase, target_len, tkind)) = target_span(target) else {
            return;
        };
        let sptr = as_void_ptr(source);
        let Some((sbase, src_len)) = resolve(sptr) else {
            __torajs_throw_type_error(
                c"the typed array's buffer is detached or out of bounds".as_ptr(),
            );
            return;
        };
        let skind = kind_of(sptr);
        // Step 13 — the one place a typed array refuses to convert.
        if skind.is_bigint() != tkind.is_bigint() {
            __torajs_throw_type_error(
                c"Cannot mix BigInt and other types, use explicit conversions".as_ptr(),
            );
            return;
        }
        let Some(off) = fits(off, src_len, target_len) else {
            return;
        };
        if src_len == 0 {
            return;
        }
        let sesize = skind.element_size() as usize;
        // Step 14 — when both views live on the same buffer the read
        // and the write can overlap, and for two different element
        // types they overlap at a different stride each side. The
        // spec clones the source region first; snapshotting whenever
        // the buffers are the same object is that, without an
        // overlap analysis that would have to be right for every
        // pair of strides.
        let snapshot: Option<Vec<u8>> = if buffer_of(as_void_ptr(target)) == buffer_of(sptr) {
            let n = src_len as usize * sesize;
            let mut v = vec![0u8; n];
            core::ptr::copy_nonoverlapping(sbase, v.as_mut_ptr(), n);
            Some(v)
        } else {
            None
        };
        let src = match &snapshot {
            Some(v) => v.as_ptr(),
            None => sbase as *const u8,
        };
        if skind == tkind {
            let n = src_len as usize * sesize;
            core::ptr::copy_nonoverlapping(src, dbase.add(off as usize * sesize), n);
            return;
        }
        for i in 0..src_len {
            let c = if skind.is_bigint() {
                Coerced::Bits(typedarray_elem::read_u64(src, i))
            } else {
                Coerced::Num(typedarray_elem::read_f64(src, skind, i))
            };
            typedarray_elem::store(dbase, tkind, off + i, c);
        }
    }
}

/// §23.2.3.26.2 SetTypedArrayFromArrayLike — `length` once, then
/// each index, and no `@@iterator` anywhere.
///
/// # Safety
/// `target` is a live TypedArray AnyValue; `source` is a live
/// AnyValue.
unsafe fn set_from_array_like(target: AnyValue, off: f64, source: AnyValue) {
    unsafe {
        let Some((_, target_len, tkind)) = target_span(target) else {
            return;
        };
        // Step 3 ToObject — the two values that have no object form.
        if source == VALUE_UNDEFINED || source == VALUE_NULL {
            __torajs_throw_type_error(c"cannot convert undefined or null to object".as_ptr());
            return;
        }
        let len_any = __torajs_any_length_get(source);
        let n = __torajs_anyv_to_number(len_any);
        __torajs_anyv_rc_dec(len_any);
        if __torajs_throw_check() != 0 {
            return;
        }
        // §7.3.18 LengthOfArrayLike is ToLength: NaN and negatives
        // are 0, so an object with no `length` sets nothing rather
        // than throwing.
        let src_len = if n.is_nan() || n <= 0.0 { 0 } else { n as i64 };
        let Some(off) = fits(off, src_len, target_len) else {
            return;
        };
        for k in 0..src_len {
            let v = __torajs_any_index_get(source, k);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(v);
                return;
            }
            let coerced = typedarray_elem::coerce(tkind, v);
            __torajs_anyv_rc_dec(v);
            let Some(c) = coerced else {
                return;
            };
            // Each Get and each coercion is user code, so the target
            // is re-derived per element. An index that has gone out
            // of range is DROPPED, not a second throw and not an
            // early exit — §10.4.5.5 discards the store and the walk
            // keeps going, because the remaining Gets are themselves
            // observable.
            if let Some((dbase, live)) = live_span(target) {
                let i = off + k;
                if i >= 0 && i < live {
                    typedarray_elem::store(dbase, tkind, i, c);
                }
            }
        }
    }
}

/// The target's extent right now, quietly — a target that has gone
/// out of bounds mid-walk makes every remaining write a no-op
/// (§10.4.5.5), it does not throw a second time.
///
/// # Safety
/// `target` is a live TypedArray AnyValue.
unsafe fn live_span(target: AnyValue) -> Option<(*mut u8, i64)> {
    unsafe { resolve(as_void_ptr(target)) }
}
