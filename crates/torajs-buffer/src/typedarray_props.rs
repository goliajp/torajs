//! §23.2.3 accessors and §10.4.5 indexed access on a typed array
//! (RFC 20260823-typedarray-substrate 刀 2).
//!
//! Every one of these re-derives the view's extent through
//! `typedarray::resolve` rather than reading a stored length. That
//! is not defensiveness — a length-tracking view over a resizable
//! buffer genuinely has no stored length, and even a fixed one can
//! go out of bounds when its buffer shrinks.
//!
//! An out-of-range index is NOT a prototype walk (§10.4.5.4): the
//! read answers `undefined` and the write is discarded. But the
//! write's COERCION still runs first (§10.4.5.5 step 1), which a
//! `valueOf` counter can see — so the discard happens after it, not
//! instead of it.

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};

use crate::typedarray::{
    ARRAY_LEN_OFF, AUTO_LENGTH, buffer_of, byte_offset_of, is_typedarray, kind_of, resolve,
};
use crate::typedarray_elem;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_rc_inc(v: AnyValue);
}

/// §23.2.3.18 `get %TypedArray%.prototype.length` — 0 for a view
/// that is out of bounds or over a detached buffer.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_length(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        unsafe { brand_error(b"length\0") };
        return 0;
    }
    unsafe { resolve(as_void_ptr(av)).map_or(0, |(_, len)| len) }
}

/// §23.2.3.2 `get %TypedArray%.prototype.byteLength`.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_byte_length(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        unsafe { brand_error(b"byteLength\0") };
        return 0;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        resolve(ptr).map_or(0, |(_, len)| len * kind_of(ptr).element_size())
    }
}

/// §23.2.3.3 `get %TypedArray%.prototype.byteOffset` — 0 once the
/// view is out of bounds, even though the stored offset is unchanged.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_byte_offset(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        unsafe { brand_error(b"byteOffset\0") };
        return 0;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        if resolve(ptr).is_none() {
            return 0;
        }
        byte_offset_of(ptr)
    }
}

/// §23.2.3.1 `get %TypedArray%.prototype.buffer` — OWNED; the view
/// keeps its own reference.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_buffer(av: AnyValue) -> AnyValue {
    if !is_typedarray(av) {
        unsafe { brand_error(b"buffer\0") };
        return VALUE_UNDEFINED;
    }
    unsafe {
        let b = buffer_of(as_void_ptr(av));
        __torajs_anyv_rc_inc(b);
        b
    }
}

/// `BYTES_PER_ELEMENT` (§23.2.5.1 table 71) read off an instance.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_bytes_per_element(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        return 0;
    }
    unsafe { kind_of(as_void_ptr(av)).element_size() }
}

/// The [`crate::typedarray::Kind`] discriminant — what the name and
/// the `@@toStringTag` are read from.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_kind(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        return -1;
    }
    unsafe { kind_of(as_void_ptr(av)) as i64 }
}

/// `true` when the view is length-tracking — the state a stored
/// length cannot express.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_tracks_length(av: AnyValue) -> i64 {
    if !is_typedarray(av) {
        return 0;
    }
    let stored = unsafe { (as_void_ptr(av).cast::<u8>().add(ARRAY_LEN_OFF) as *const i64).read() };
    i64::from(stored == AUTO_LENGTH)
}

/// `x instanceof <T>` — the eleven constructors share one heap tag,
/// so identity is the element kind and not the tag.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_is_kind(av: AnyValue, kind: i64) -> bool {
    if !is_typedarray(av) {
        return false;
    }
    unsafe { kind_of(as_void_ptr(av)) as i64 == kind }
}

/// §10.4.5.4 [[Get]] for an integer index. An index that is not a
/// valid one answers `undefined` and does NOT continue up the
/// prototype chain — that is what makes a typed array an integer-
/// indexed exotic object rather than an object with numeric keys.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_index_get(av: AnyValue, index: f64) -> AnyValue {
    if !is_typedarray(av) {
        return VALUE_UNDEFINED;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        let Some((base, len)) = resolve(ptr) else {
            return VALUE_UNDEFINED;
        };
        let Some(i) = valid_index(index, len) else {
            return VALUE_UNDEFINED;
        };
        typedarray_elem::read(base, kind_of(ptr), i)
    }
}

/// §10.4.5.5 TypedArraySetElement. The coercion in step 1 runs
/// whether or not the index is valid, so a `valueOf` fires even for
/// a write that lands nowhere.
///
/// # Safety
/// `av` and `value` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_index_set(av: AnyValue, index: f64, value: AnyValue) {
    if !is_typedarray(av) {
        return;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        let kind = kind_of(ptr);
        // Step 1 first, unconditionally — and it can detach or
        // resize the buffer, which is why the extent is asked for
        // only afterwards.
        let Some(c) = typedarray_elem::coerce(kind, value) else {
            return;
        };
        let Some((base, len)) = resolve(ptr) else {
            return;
        };
        if let Some(i) = valid_index(index, len) {
            typedarray_elem::store(base, kind, i, c);
        }
    }
}

/// §10.4.5's IsValidIntegerIndex, over the Number the read actually
/// carries: an integral value in `[0, len)`, and `-0` is not one.
fn valid_index(index: f64, len: i64) -> Option<i64> {
    if !index.is_finite() || index.trunc() != index {
        return None;
    }
    if index == 0.0 && index.is_sign_negative() {
        return None;
    }
    if index < 0.0 || index >= len as f64 {
        return None;
    }
    Some(index as i64)
}

/// # Safety
/// Records a pending throw.
unsafe fn brand_error(name: &[u8]) {
    let mut msg = [0u8; 96];
    let head = b"this is not a typed array: ";
    msg[..head.len()].copy_from_slice(head);
    let n = name.len().min(msg.len() - head.len());
    msg[head.len()..head.len() + n].copy_from_slice(&name[..n]);
    unsafe { __torajs_throw_type_error(msg.as_ptr() as *const core::ffi::c_char) };
}
