//! §10.4.5 element access — the byte-level half of a typed array
//! (RFC 20260823-typedarray-substrate 刀 2).
//!
//! Reading is a reinterpretation of the bytes. Writing is the §7.1
//! coercion the element type names, and those are NOT
//! interchangeable: the six integer kinds truncate then wrap modulo
//! their width, `Uint8ClampedArray` clamps and rounds half to EVEN,
//! and the two 64-bit integer kinds take `ToBigInt` and reject a
//! Number outright. Getting `Uint8Clamped` wrong is invisible on
//! every value except the exact halves.
//!
//! Every access is unaligned by construction — a view can start at
//! any byte offset of its buffer — so all of it goes through
//! `read_unaligned` / `write_unaligned`.

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, box_double, box_void_ptr};

use crate::typedarray::Kind;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    /// §7.1.13 ToBigInt — answers a BigInt cell, or records a
    /// TypeError and answers null for a value that has no BigInt.
    fn __torajs_any_to_bigint(v: u64) -> *mut u8;
    fn __torajs_bigint_from_i64(v: i64) -> *mut u8;
    /// The unsigned mint — `from_i64` cannot express the half of
    /// `BigUint64Array`'s range above `i64::MAX`.
    fn __torajs_bigint_from_u64(v: u64) -> *mut u8;
    /// The low 64 bits of a BigInt's two's-complement value —
    /// §7.1.x `ToBigInt64` / `ToBigUint64` modulo, which is the same
    /// bit pattern for both.
    fn __torajs_bigint_to_u64_wrapping(p: *const c_void) -> u64;
    fn __torajs_bigint_drop(p: *mut c_void);
}

/// §7.1.5-ish truncation shared by the six wrapping integer kinds:
/// NaN and the infinities are 0, everything else truncates toward
/// zero and then wraps modulo `2^bits`.
fn wrap_to_u64(n: f64, bits: u32) -> u64 {
    if !n.is_finite() {
        return 0;
    }
    let modulus = 2f64.powi(bits as i32);
    let t = n.trunc();
    // `rem_euclid` lands in `[0, modulus)`, which is exactly the
    // unsigned answer; the signed kinds reinterpret those same bits.
    let m = t.rem_euclid(modulus);
    m as u64
}

/// §7.1.11 ToUint8Clamp — the only conversion in the family that
/// ROUNDS rather than truncates, and it rounds halves to even. The
/// difference is visible on exactly the half-integers, which is why
/// a truncating stand-in survives most tests.
fn to_uint8_clamp(n: f64) -> u8 {
    if n.is_nan() || n <= 0.0 {
        return 0;
    }
    if n >= 255.0 {
        return 255;
    }
    let f = n.floor();
    if f + 0.5 < n {
        return (f as u8).wrapping_add(1);
    }
    if n < f + 0.5 {
        return f as u8;
    }
    // Exactly halfway — pick the even neighbour.
    if (f as u64) % 2 == 1 {
        (f as u8).wrapping_add(1)
    } else {
        f as u8
    }
}

/// Read element `i` of a view whose data starts at `base`.
///
/// # Safety
/// `base` addresses at least `(i + 1) * kind.element_size()` bytes.
pub(crate) unsafe fn read(base: *const u8, kind: Kind, i: i64) -> AnyValue {
    let p = unsafe { base.add((i * kind.element_size()) as usize) };
    unsafe {
        match kind {
            Kind::Int8 => box_double(f64::from(p.cast::<i8>().read_unaligned())),
            Kind::Uint8 | Kind::Uint8Clamped => box_double(f64::from(p.read_unaligned())),
            Kind::Int16 => box_double(f64::from(p.cast::<i16>().read_unaligned())),
            Kind::Uint16 => box_double(f64::from(p.cast::<u16>().read_unaligned())),
            Kind::Int32 => box_double(f64::from(p.cast::<i32>().read_unaligned())),
            Kind::Uint32 => box_double(f64::from(p.cast::<u32>().read_unaligned())),
            Kind::Float16 => box_double(crate::binary16::f16_bits_to_f64(
                p.cast::<u16>().read_unaligned(),
            )),
            Kind::Float32 => box_double(f64::from(p.cast::<f32>().read_unaligned())),
            Kind::Float64 => box_double(p.cast::<f64>().read_unaligned()),
            Kind::BigInt64 => {
                let cell = __torajs_bigint_from_i64(p.cast::<i64>().read_unaligned());
                box_void_ptr(cell as *mut c_void)
            }
            Kind::BigUint64 => {
                let cell = __torajs_bigint_from_u64(p.cast::<u64>().read_unaligned());
                box_void_ptr(cell as *mut c_void)
            }
        }
    }
}

/// The element as an `f64`, for the ten kinds that are Numbers.
///
/// This is exact for every one of them — the widest integer kind is
/// 32 bits and the widest float is `f64` itself — so a comparison
/// done here answers what a comparison against the boxed value would
/// have, without minting anything. The two BigInt kinds are not
/// Numbers and go through [`read_u64`] instead.
///
/// # Safety
/// `base` addresses at least `(i + 1) * kind.element_size()` bytes,
/// and `kind` is not one of the two BigInt kinds.
pub(crate) unsafe fn read_f64(base: *const u8, kind: Kind, i: i64) -> f64 {
    let p = unsafe { base.add((i * kind.element_size()) as usize) };
    unsafe {
        match kind {
            Kind::Int8 => f64::from(p.cast::<i8>().read_unaligned()),
            Kind::Uint8 | Kind::Uint8Clamped => f64::from(p.read_unaligned()),
            Kind::Int16 => f64::from(p.cast::<i16>().read_unaligned()),
            Kind::Uint16 => f64::from(p.cast::<u16>().read_unaligned()),
            Kind::Int32 => f64::from(p.cast::<i32>().read_unaligned()),
            Kind::Uint32 => f64::from(p.cast::<u32>().read_unaligned()),
            Kind::Float16 => crate::binary16::f16_bits_to_f64(p.cast::<u16>().read_unaligned()),
            Kind::Float32 => f64::from(p.cast::<f32>().read_unaligned()),
            Kind::Float64 => p.cast::<f64>().read_unaligned(),
            Kind::BigInt64 | Kind::BigUint64 => unreachable!("a BigInt element is not a Number"),
        }
    }
}

/// The raw 64 bits of a BigInt element. Two BigInts are equal iff
/// their two's-complement bit patterns are, once both are known to
/// be in the element type's range — which is what the caller
/// establishes for the needle before it starts scanning.
///
/// # Safety
/// `base` addresses at least `(i + 1) * 8` bytes.
pub(crate) unsafe fn read_u64(base: *const u8, i: i64) -> u64 {
    unsafe { base.add((i * 8) as usize).cast::<u64>().read_unaligned() }
}

/// The coerced form of a value on its way into an element — §7.1's
/// answer, held between step 1 of §10.4.5.5 (which always runs) and
/// the store (which may not happen at all).
#[derive(Clone, Copy)]
pub(crate) enum Coerced {
    Num(f64),
    Bits(u64),
}

/// §10.4.5.5 step 1 — run the coercion the element type names.
/// `None` means it threw and the pending throw is recorded.
///
/// This is deliberately separate from the store: the spec coerces
/// BEFORE deciding whether the index is valid, so a write that lands
/// nowhere still fires its `valueOf`.
///
/// # Safety
/// `v` is a live AnyValue.
pub(crate) unsafe fn coerce(kind: Kind, v: AnyValue) -> Option<Coerced> {
    unsafe {
        if kind.is_bigint() {
            let cell = __torajs_any_to_bigint(v);
            if cell.is_null() || __torajs_throw_check() != 0 {
                if !cell.is_null() {
                    __torajs_bigint_drop(cell as *mut c_void);
                }
                return None;
            }
            let raw = __torajs_bigint_to_u64_wrapping(cell as *const c_void);
            __torajs_bigint_drop(cell as *mut c_void);
            return Some(Coerced::Bits(raw));
        }
        let n = __torajs_anyv_to_number(v);
        if __torajs_throw_check() != 0 {
            return None;
        }
        Some(Coerced::Num(n))
    }
}

/// Store an already-coerced value into element `i`.
///
/// # Safety
/// `base` addresses at least `(i + 1) * kind.element_size()` bytes,
/// and `c` came from [`coerce`] with the same `kind`.
pub(crate) unsafe fn store(base: *mut u8, kind: Kind, i: i64, c: Coerced) {
    unsafe {
        let p = base.add((i * kind.element_size()) as usize);
        match (kind, c) {
            (Kind::BigInt64 | Kind::BigUint64, Coerced::Bits(raw)) => {
                p.cast::<u64>().write_unaligned(raw)
            }
            (Kind::Int8 | Kind::Uint8, Coerced::Num(n)) => {
                p.write_unaligned(wrap_to_u64(n, 8) as u8)
            }
            (Kind::Uint8Clamped, Coerced::Num(n)) => p.write_unaligned(to_uint8_clamp(n)),
            (Kind::Int16 | Kind::Uint16, Coerced::Num(n)) => {
                p.cast::<u16>().write_unaligned(wrap_to_u64(n, 16) as u16)
            }
            (Kind::Int32 | Kind::Uint32, Coerced::Num(n)) => {
                p.cast::<u32>().write_unaligned(wrap_to_u64(n, 32) as u32)
            }
            (Kind::Float16, Coerced::Num(n)) => p
                .cast::<u16>()
                .write_unaligned(crate::binary16::f64_to_f16_bits(n)),
            (Kind::Float32, Coerced::Num(n)) => p.cast::<f32>().write_unaligned(n as f32),
            (Kind::Float64, Coerced::Num(n)) => p.cast::<f64>().write_unaligned(n),
            // `coerce` answers Bits for exactly the BigInt kinds and
            // Num for exactly the others, so no pair is reachable.
            _ => unreachable!("coerced form does not match the element kind"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_matches_the_to_intn_family() {
        assert_eq!(wrap_to_u64(300.0, 8), 44);
        assert_eq!(wrap_to_u64(-1.0, 8), 255);
        assert_eq!(wrap_to_u64(-1.0, 32), 4294967295);
        assert_eq!(wrap_to_u64(f64::NAN, 8), 0);
        assert_eq!(wrap_to_u64(f64::INFINITY, 16), 0);
        assert_eq!(wrap_to_u64(f64::NEG_INFINITY, 16), 0);
        assert_eq!(wrap_to_u64(3.9, 8), 3);
        assert_eq!(wrap_to_u64(-3.9, 8), 253);
    }

    #[test]
    fn clamping_rounds_halves_to_even() {
        assert_eq!(to_uint8_clamp(f64::NAN), 0);
        assert_eq!(to_uint8_clamp(-5.0), 0);
        assert_eq!(to_uint8_clamp(300.0), 255);
        assert_eq!(to_uint8_clamp(1.4), 1);
        assert_eq!(to_uint8_clamp(1.6), 2);
        // The halves: 0.5 → 0, 1.5 → 2, 2.5 → 2, 3.5 → 4.
        assert_eq!(to_uint8_clamp(0.5), 0);
        assert_eq!(to_uint8_clamp(1.5), 2);
        assert_eq!(to_uint8_clamp(2.5), 2);
        assert_eq!(to_uint8_clamp(3.5), 4);
        assert_eq!(to_uint8_clamp(254.5), 254);
    }
}
