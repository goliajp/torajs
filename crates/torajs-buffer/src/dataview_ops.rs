//! §25.3.4 `DataView.prototype.get*` / `set*` — the byte-granular
//! accessor methods (RFC 20260823-typedarray-substrate 刀 7).
//!
//! One get kernel and one set kernel, keyed by the element-kind
//! discriminant the method NAME resolved to at compile time — the
//! same shape the eleven typed-array constructors share. Endianness
//! is a per-call argument here, not a property of the view, which is
//! the whole reason DataView exists next to the typed arrays.
//!
//! §25.3.1.1 GetViewValue / SetViewValue step order is the thing to
//! preserve: `ToIndex(requestIndex)` and (for a set) the value
//! coercion both run BEFORE the buffer is inspected, and either can
//! run user code that detaches or resizes it.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_double};

use crate::dataview::{is_dataview, resolve};
use crate::typedarray::Kind;
use crate::typedarray_elem::{Coerced, coerce};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_to_index(n: f64) -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    fn __torajs_anyv_to_bool(v: AnyValue) -> bool;
    fn __torajs_bigint_from_i64(v: i64) -> *mut u8;
    fn __torajs_bigint_from_u64(v: u64) -> *mut u8;
    fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue;
}

/// §7.1.22 ToIndex over the request-index slot; `None` = it threw.
unsafe fn to_request_index(av: AnyValue) -> Option<i64> {
    unsafe {
        let n = if av == VALUE_UNDEFINED {
            0.0
        } else {
            let n = __torajs_anyv_to_number(av);
            if __torajs_throw_check() != 0 {
                return None;
            }
            n
        };
        let i = __torajs_to_index(n);
        if __torajs_throw_check() != 0 {
            return None;
        }
        Some(i)
    }
}

/// The view's window for a `size`-byte access at `index`, after the
/// §25.3.1.1 bounds discipline: `None` = the TypeError or RangeError
/// is recorded and the caller answers undefined.
unsafe fn access_ptr(recv: AnyValue, index: i64, size: i64) -> Option<*mut u8> {
    unsafe {
        let Some((data, view_len)) = resolve(as_void_ptr(recv)) else {
            __torajs_throw_type_error(
                c"DataView operation called on an invalid or out-of-bounds view".as_ptr(),
            );
            return None;
        };
        if index + size > view_len {
            __torajs_throw_range_error(b"Out of bounds access\0".as_ptr());
            return None;
        }
        Some(data.add(index as usize))
    }
}

/// Raw bytes of one element, normalized to LITTLE-endian order
/// regardless of what the call asked for.
unsafe fn load_le(p: *const u8, size: usize, little: bool) -> [u8; 8] {
    let mut b = [0u8; 8];
    unsafe { core::ptr::copy_nonoverlapping(p, b.as_mut_ptr(), size) };
    if !little {
        b[..size].reverse();
    }
    b
}

/// §25.3.4's shared get body. `kind` is the element discriminant
/// (never `Uint8Clamped` — DataView has no clamped accessor).
///
/// # Safety
/// The argument slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_get(
    recv: AnyValue,
    kind: i64,
    offset: AnyValue,
    little_endian: AnyValue,
) -> AnyValue {
    unsafe {
        if !is_dataview(recv) {
            __torajs_throw_type_error(c"DataView method called on a non-DataView".as_ptr());
            return VALUE_UNDEFINED;
        }
        let kind = Kind::from_repr(kind as u8);
        let Some(index) = to_request_index(offset) else {
            return VALUE_UNDEFINED;
        };
        let little = __torajs_anyv_to_bool(little_endian);
        let size = kind.element_size();
        let Some(p) = access_ptr(recv, index, size) else {
            return VALUE_UNDEFINED;
        };
        let b = load_le(p, size as usize, little);
        match kind {
            Kind::Int8 => box_double(f64::from(b[0] as i8)),
            Kind::Uint8 | Kind::Uint8Clamped => box_double(f64::from(b[0])),
            Kind::Int16 => box_double(f64::from(i16::from_le_bytes([b[0], b[1]]))),
            Kind::Uint16 => box_double(f64::from(u16::from_le_bytes([b[0], b[1]]))),
            Kind::Int32 => box_double(f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            Kind::Uint32 => box_double(f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            Kind::Float16 => box_double(crate::binary16::f16_bits_to_f64(u16::from_le_bytes([
                b[0], b[1],
            ]))),
            Kind::Float32 => box_double(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
            Kind::Float64 => box_double(f64::from_le_bytes(b)),
            Kind::BigInt64 => __torajs_anyv_box_pointer(__torajs_bigint_from_i64(
                i64::from_le_bytes(b),
            ) as *mut c_void),
            Kind::BigUint64 => __torajs_anyv_box_pointer(__torajs_bigint_from_u64(
                u64::from_le_bytes(b),
            ) as *mut c_void),
        }
    }
}

/// §25.3.4's shared set body — answers nothing; a recorded throw is
/// the only other outcome.
///
/// # Safety
/// The argument slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_set(
    recv: AnyValue,
    kind: i64,
    offset: AnyValue,
    value: AnyValue,
    little_endian: AnyValue,
) {
    unsafe {
        if !is_dataview(recv) {
            __torajs_throw_type_error(c"DataView method called on a non-DataView".as_ptr());
            return;
        }
        let kind = Kind::from_repr(kind as u8);
        let Some(index) = to_request_index(offset) else {
            return;
        };
        // §25.3.1.2 SetViewValue step 3 — the value coercion runs
        // before the buffer is measured (its `valueOf` can detach).
        let Some(c) = coerce(kind, value) else {
            return;
        };
        let little = __torajs_anyv_to_bool(little_endian);
        let size = kind.element_size();
        let Some(p) = access_ptr(recv, index, size) else {
            return;
        };
        let mut b = [0u8; 8];
        match (kind, c) {
            (Kind::BigInt64 | Kind::BigUint64, Coerced::Bits(raw)) => {
                b = raw.to_le_bytes();
            }
            (Kind::Int8 | Kind::Uint8 | Kind::Uint8Clamped, Coerced::Num(n)) => {
                b[0] = crate::typedarray_elem::wrap_to_u64(n, 8) as u8;
            }
            (Kind::Int16 | Kind::Uint16, Coerced::Num(n)) => {
                b[..2].copy_from_slice(
                    &(crate::typedarray_elem::wrap_to_u64(n, 16) as u16).to_le_bytes(),
                );
            }
            (Kind::Int32 | Kind::Uint32, Coerced::Num(n)) => {
                b[..4].copy_from_slice(
                    &(crate::typedarray_elem::wrap_to_u64(n, 32) as u32).to_le_bytes(),
                );
            }
            (Kind::Float16, Coerced::Num(n)) => {
                b[..2].copy_from_slice(&crate::binary16::f64_to_f16_bits(n).to_le_bytes());
            }
            (Kind::Float32, Coerced::Num(n)) => {
                b[..4].copy_from_slice(&(n as f32).to_le_bytes());
            }
            (Kind::Float64, Coerced::Num(n)) => {
                b = n.to_le_bytes();
            }
            // `coerce` answers Bits for exactly the BigInt kinds and
            // Num for exactly the others.
            _ => unreachable!("coerced form does not match the element kind"),
        }
        if !little {
            b[..size as usize].reverse();
        }
        core::ptr::copy_nonoverlapping(b.as_ptr(), p, size as usize);
    }
}
