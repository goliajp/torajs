//! The ArrayBuffer methods that move bytes — §25.1.6.6 `resize`,
//! §25.1.6.7 `slice` (RFC 20260823-typedarray-substrate 刀 1), and
//! §25.1.6.7-8 `transfer` / `transferToFixedLength` (刀 8).
//!
//! Split from `arraybuffer.rs` along the seam that file already has:
//! the cell and its getters *answer questions about* a buffer, these
//! two *change or copy* one.

use core::ffi::c_char;

use torajs_anyvalue::__torajs_anyv_box_pointer;
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};

use crate::arraybuffer::{
    BYTE_LEN_OFF, NOT_RESIZABLE, allocate, byte_len, data_ptr, is_arraybuffer, max_byte_len,
};

unsafe extern "C" {
    /// torajs-anyvalue — §7.3.20 species constructor-face guard
    /// (`buffer_species.rs`).
    fn __torajs_buffer_species_guard(recv: AnyValue) -> i64;
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_to_index(n: f64) -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
}

/// §7.1.5 ToIntegerOrInfinity, as far as a byte offset can see it:
/// NaN is +0 and the infinities survive as themselves so the clamp
/// below can send them to the right end.
fn to_integer_or_infinity(n: f64) -> f64 {
    if n.is_nan() { 0.0 } else { n.trunc() }
}

/// The relative-index clamp both `slice` bounds use (§25.1.6.7
/// steps 6 and 8): negative counts from the end, and everything
/// lands inside `[0, len]`.
fn clamp_relative(rel: f64, len: i64) -> i64 {
    if rel < 0.0 {
        let from_end = len as f64 + rel;
        if from_end < 0.0 { 0 } else { from_end as i64 }
    } else if rel > len as f64 {
        len
    } else {
        rel as i64
    }
}

/// §25.1.6.6 `ArrayBuffer.prototype.resize(newLength)`.
///
/// Step order is load-bearing and is not the order a reader would
/// guess: the *resizable* check comes before `ToIndex` (a
/// fixed-length buffer rejects without ever coercing the argument),
/// and the *detached* check comes after it (because `ToIndex` can run
/// user code that detaches the buffer).
///
/// The byte store is never reallocated — the maximum was reserved at
/// construction. Shrinking zeroes what it hides, which is what keeps
/// the invariant "every byte at or above `byte_len` is zero" true for
/// the next grow.
///
/// # Safety
/// `av` and `new_length` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_resize(av: AnyValue, new_length: AnyValue) {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.resize called on incompatible receiver".as_ptr(),
            )
        };
        return;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        let max = max_byte_len(ptr);
        if max == NOT_RESIZABLE {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.resize: buffer is not resizable".as_ptr(),
            );
            return;
        }
        let n = __torajs_anyv_to_number(new_length);
        if __torajs_throw_check() != 0 {
            return;
        }
        let new_len = __torajs_to_index(n);
        if __torajs_throw_check() != 0 {
            return;
        }
        if data_ptr(ptr).is_null() {
            __torajs_throw_type_error(c"ArrayBuffer.prototype.resize: buffer is detached".as_ptr());
            return;
        }
        if new_len > max {
            __torajs_throw_range_error(
                b"ArrayBuffer.prototype.resize: newLength exceeds maxByteLength\0".as_ptr(),
            );
            return;
        }
        let old_len = byte_len(ptr);
        if new_len < old_len {
            let data = data_ptr(ptr);
            core::ptr::write_bytes(data.add(new_len as usize), 0, (old_len - new_len) as usize);
        }
        *(ptr.cast::<u8>().add(BYTE_LEN_OFF) as *mut i64) = new_len;
    }
}

/// §25.1.6.7 `ArrayBuffer.prototype.slice(start, end)` — a fresh
/// fixed-length buffer holding the copied range.
///
/// **Boundary, recorded not hidden**: step 10 reads
/// `SpeciesConstructor(O, %ArrayBuffer%)`, so a subclass can hand
/// back a buffer of its own kind. This builds `%ArrayBuffer%`
/// directly; species dispatch arrives with the rest of `@@species`.
///
/// # Safety
/// `av`, `start` and `end` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_slice(
    av: AnyValue,
    start: AnyValue,
    end: AnyValue,
) -> AnyValue {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.slice called on incompatible receiver".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        if data_ptr(ptr).is_null() {
            __torajs_throw_type_error(c"ArrayBuffer.prototype.slice: buffer is detached".as_ptr());
            return VALUE_UNDEFINED;
        }
        let len = byte_len(ptr);
        let first = {
            let n = __torajs_anyv_to_number(start);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            clamp_relative(to_integer_or_infinity(n), len)
        };
        let last = if end == VALUE_UNDEFINED {
            len
        } else {
            let n = __torajs_anyv_to_number(end);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            clamp_relative(to_integer_or_infinity(n), len)
        };
        // §25.1.6.16 step 14 — the species constructor-face read
        // (`species.rs`); a poisoned instance `constructor` throws
        // before any allocation.
        if __torajs_buffer_species_guard(av) != 0 {
            return VALUE_UNDEFINED;
        }
        // The coercions above can run user code, so the source may
        // have been detached (or shrunk) since the length was read.
        let src = data_ptr(ptr);
        if src.is_null() {
            __torajs_throw_type_error(c"ArrayBuffer.prototype.slice: buffer is detached".as_ptr());
            return VALUE_UNDEFINED;
        }
        let new_len = (last - first).max(0);
        let cell = allocate(new_len, NOT_RESIZABLE);
        if cell.is_null() {
            __torajs_throw_range_error(b"ArrayBuffer allocation failed\0".as_ptr());
            return VALUE_UNDEFINED;
        }
        let copy = new_len.min((byte_len(ptr) - first).max(0));
        if copy > 0 {
            core::ptr::copy_nonoverlapping(src.add(first as usize), data_ptr(cell), copy as usize);
        }
        __torajs_anyv_box_pointer(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_counts_from_the_end_and_stays_in_range() {
        assert_eq!(clamp_relative(-2.0, 8), 6);
        assert_eq!(clamp_relative(-99.0, 8), 0);
        assert_eq!(clamp_relative(3.0, 8), 3);
        assert_eq!(clamp_relative(99.0, 8), 8);
        assert_eq!(clamp_relative(f64::INFINITY, 8), 8);
        assert_eq!(clamp_relative(f64::NEG_INFINITY, 8), 0);
    }

    #[test]
    fn nan_is_zero_and_truncation_is_toward_zero() {
        assert_eq!(to_integer_or_infinity(f64::NAN), 0.0);
        assert_eq!(to_integer_or_infinity(-3.9), -3.0);
        assert_eq!(to_integer_or_infinity(3.9), 3.0);
    }
}

/// §25.1.6.7 `ArrayBuffer.prototype.transfer(newLength)` and
/// §25.1.6.8 `transferToFixedLength(newLength)` — one body, because
/// the spec gives them one (AbstractSetBufferMaxByteLength calls the
/// same steps with `preserve-resizability` set either way).
///
/// This is the only way a program detaches a buffer: the bytes move
/// to a fresh cell and the old one is emptied. test262 reaches for it
/// through `$DETACHBUFFER`, which is why so many typed-array cases
/// sit behind it.
///
/// Step order matters twice. `ToIndex` runs BEFORE the detached
/// check (step 3 before step 4), so `detached.transfer({valueOf(){…}})`
/// runs the user code and only then reports the buffer. And the
/// allocation happens before the detach, so a failed reservation
/// leaves the receiver intact rather than half-transferred.
///
/// # Safety
/// `av` and `new_length` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_transfer(
    av: AnyValue,
    new_length: AnyValue,
    preserve_resizability: i64,
) -> AnyValue {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.transfer called on incompatible receiver".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        let old_len = byte_len(ptr);
        // Step 3 — an absent newLength keeps the current length; a
        // present one coerces, and that can run user code.
        let new_len = if torajs_anyvalue::nanbox::is_undefined(new_length) {
            old_len
        } else {
            let n = __torajs_anyv_to_number(new_length);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let idx = __torajs_to_index(n);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            idx
        };
        // Step 4 — after the coercion, which may have detached it.
        if data_ptr(ptr).is_null() {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.transfer: buffer is detached".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // Step 5 — `transfer` keeps a resizable buffer resizable;
        // `transferToFixedLength` is the same steps with that turned
        // off, which is the whole difference between the two names.
        let old_max = max_byte_len(ptr);
        let new_max = if preserve_resizability != 0 && old_max != NOT_RESIZABLE {
            old_max
        } else {
            NOT_RESIZABLE
        };
        if new_max != NOT_RESIZABLE && new_len > new_max {
            __torajs_throw_range_error(
                b"ArrayBuffer.prototype.transfer: newLength exceeds maxByteLength\0".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let fresh = allocate(new_len, new_max);
        if fresh.is_null() {
            __torajs_throw_range_error(
                b"ArrayBuffer.prototype.transfer: allocation failed\0".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // Steps 8-9 — the shorter of the two lengths travels; a grow
        // reads the fresh cell's zeroed tail.
        let copied = if new_len < old_len { new_len } else { old_len };
        if copied > 0 {
            core::ptr::copy_nonoverlapping(data_ptr(ptr), data_ptr(fresh), copied as usize);
        }
        crate::arraybuffer::detach(ptr);
        __torajs_anyv_box_pointer(fresh)
    }
}
