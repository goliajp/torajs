//! §23.2.5.1 `new <TypedArray>(…)` — the two construction forms this
//! knife builds (RFC 20260823-typedarray-substrate 刀 2).
//!
//! - `new T(length)` (and `new T()`) allocates a private buffer;
//! - `new T(buffer [, byteOffset [, length ]])` views an existing one.
//!
//! The remaining three forms — from another typed array, from an
//! iterable, from an array-like — are 刀 3; each of them reads
//! elements through machinery that does not exist yet, and a
//! half-built one would be worse than the loud reject they get now.
//!
//! §23.2.5.1.5's step order is the thing to preserve: the offset
//! coercion runs BEFORE the detached check, the length coercion
//! before it too, and both can run user code that detaches or
//! resizes the buffer — so the buffer's length is read only after
//! both are done.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::__torajs_anyv_box_pointer;
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};
use torajs_rc::Tag;

use crate::arraybuffer::{
    NOT_RESIZABLE, allocate, byte_len, data_ptr, is_arraybuffer, max_byte_len,
};
use crate::typedarray::{
    ARRAY_LEN_OFF, AUTO_LENGTH, BUFFER_OFF, BYTE_OFFSET_OFF, CELL_SIZE, KIND_OFF, Kind,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_to_index(n: f64) -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    fn __torajs_anyv_rc_inc(v: AnyValue);
    fn __torajs_anyv_rc_dec(v: AnyValue);
}

/// Mint a view cell over `buffer`, which the cell takes its own
/// reference to. `array_len` is [`AUTO_LENGTH`] for a tracking view.
///
/// # Safety
/// `buffer` is a live ArrayBuffer AnyValue.
pub(crate) unsafe fn mint(
    kind: Kind,
    buffer: AnyValue,
    byte_offset: i64,
    array_len: i64,
) -> AnyValue {
    unsafe {
        __torajs_anyv_rc_inc(buffer);
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::TypedArray as u16;
        *(cell.add(BUFFER_OFF) as *mut u64) = buffer;
        *(cell.add(BYTE_OFFSET_OFF) as *mut i64) = byte_offset;
        *(cell.add(ARRAY_LEN_OFF) as *mut i64) = array_len;
        *cell.add(KIND_OFF) = kind as u8;
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// `value_drop`'s TypedArray arm — release the buffer and free.
///
/// # Safety
/// `cell` is a live TypedArray cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            return;
        }
        __torajs_anyv_rc_dec((cell.cast::<u8>().add(BUFFER_OFF) as *const u64).read());
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(cell.cast::<u8>(), layout);
    }
}

/// §23.2.5.1. `kind` is the [`Kind`] discriminant the lowering
/// resolved from the constructor name; the three argument slots are
/// BORROWED and a missing one is `undefined`.
///
/// # Safety
/// The argument slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_create(
    kind: i64,
    a0: AnyValue,
    a1: AnyValue,
    a2: AnyValue,
) -> AnyValue {
    let kind = Kind::from_repr(kind as u8);
    if is_arraybuffer(a0) {
        return unsafe { from_buffer(kind, a0, a1, a2) };
    }
    if torajs_anyvalue::nanbox::is_cell(a0) {
        // §23.2.5.1 steps 5.b-5.d — another typed array, an
        // iterable, or an array-like. 刀 3; a loud reject beats a
        // half-built one that answers zeros.
        unsafe {
            __torajs_throw_type_error(
                c"TypedArray construction from an object is not yet supported".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    unsafe { from_length(kind, a0) }
}

/// §23.2.5.1 step 4 — `new T(len)` allocates its own buffer.
///
/// # Safety
/// `a0` is a live AnyValue.
unsafe fn from_length(kind: Kind, a0: AnyValue) -> AnyValue {
    unsafe {
        let len = if a0 == VALUE_UNDEFINED {
            0
        } else {
            let n = __torajs_anyv_to_number(a0);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let l = __torajs_to_index(n);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            l
        };
        let Some(bytes) = len.checked_mul(kind.element_size()) else {
            __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
            return VALUE_UNDEFINED;
        };
        let buf_cell = allocate(bytes, NOT_RESIZABLE);
        if buf_cell.is_null() {
            __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
            return VALUE_UNDEFINED;
        }
        let buffer = __torajs_anyv_box_pointer(buf_cell);
        let view = mint(kind, buffer, 0, len);
        // `mint` took its own reference; the local box is the
        // allocation's only other one.
        __torajs_anyv_rc_dec(buffer);
        view
    }
}

/// §23.2.5.1.5 InitializeTypedArrayFromArrayBuffer.
///
/// # Safety
/// `buffer` is a live ArrayBuffer AnyValue; the other slots are live
/// AnyValues.
unsafe fn from_buffer(kind: Kind, buffer: AnyValue, a1: AnyValue, a2: AnyValue) -> AnyValue {
    unsafe {
        let esize = kind.element_size();
        // Step 2 — both coercions run before anything is checked
        // against the buffer, and either can detach or resize it.
        let offset = {
            let n = __torajs_anyv_to_number(a1);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let o = if a1 == VALUE_UNDEFINED {
                0
            } else {
                __torajs_to_index(n)
            };
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            o
        };
        if offset % esize != 0 {
            __torajs_throw_range_error(
                b"start offset is not a multiple of BYTES_PER_ELEMENT\0".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let explicit_len = if a2 == VALUE_UNDEFINED {
            None
        } else {
            let n = __torajs_anyv_to_number(a2);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let l = __torajs_to_index(n);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            Some(l)
        };
        // Step 6 — only NOW is the buffer inspected.
        let bptr = as_void_ptr(buffer);
        if data_ptr(bptr).is_null() {
            __torajs_throw_type_error(
                c"Cannot construct a typed array on a detached buffer".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let buf_len = byte_len(bptr);
        let tracking = max_byte_len(bptr) != NOT_RESIZABLE;
        let array_len = match explicit_len {
            // Step 8 — a length-tracking view over a resizable
            // buffer stores no length at all.
            None if tracking => {
                if offset > buf_len {
                    __torajs_throw_range_error(
                        b"start offset is outside the bounds of the buffer\0".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                AUTO_LENGTH
            }
            // Step 9 — a fixed-length buffer with no explicit length
            // takes the whole remainder, which must divide evenly.
            None => {
                if buf_len % esize != 0 {
                    __torajs_throw_range_error(
                        b"byte length is not a multiple of BYTES_PER_ELEMENT\0".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                if offset > buf_len {
                    __torajs_throw_range_error(
                        b"start offset is outside the bounds of the buffer\0".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                (buf_len - offset) / esize
            }
            // Step 10.
            Some(len) => {
                let Some(bytes) = len.checked_mul(esize) else {
                    __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
                    return VALUE_UNDEFINED;
                };
                if offset + bytes > buf_len {
                    __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
                    return VALUE_UNDEFINED;
                }
                len
            }
        };
        mint(kind, buffer, offset, array_len)
    }
}
