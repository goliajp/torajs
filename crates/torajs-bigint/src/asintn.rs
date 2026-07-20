//! `BigInt.asIntN(bits, value)` / `BigInt.asUintN(bits, value)` per
//! ES §21.2.2.1 / §21.2.2.2.
//!
//! Modular truncation to a fixed-width view:
//!
//! - `asUintN(bits, x)`  = `x mod 2^bits`                 (unsigned)
//! - `asIntN(bits, x)`   = `((x + 2^(bits-1)) mod 2^bits) - 2^(bits-1)`
//!                       = `asUintN(bits, x)` reinterpreted as
//!                         two's-complement signed.
//!
//! Arbitrary `bits` is supported via word-level masking + multi-limb
//! two's-complement propagation. Fast path: when the input magnitude
//! already fits the requested width, the result is the input itself
//! (fresh clone — no per-limb rework).
//!
//! Resource bound: `asUintN(bits, negative)` materializes a result
//! that genuinely needs `bits` bits (`2^bits - |x| mod 2^bits`), so
//! `bits` beyond [`MAX_BITS`] throws the implementation-defined
//! "Maximum BigInt size exceeded" `RangeError` (same cap V8 uses).
//! All other paths allocate at most the input's own limb count.

use core::ffi::c_void;

use crate::internal::{
    alloc_raw, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

/// Implementation cap on BigInt bit width (matches V8's
/// `kMaxLengthBits`). Only reachable from `asUintN(huge, negative)`.
const MAX_BITS: i64 = 1 << 30;

unsafe extern "C" {
    /// Resolved at link time to torajs-throw's record-pending-throw
    /// helper. The IR-emit layer in the SSA caller is responsible for
    /// translating "pending throw set" into the user-visible
    /// `RangeError` via `emit_throw_check`.
    fn __torajs_throw_range_error(msg: *const u8);
}

// `__torajs_throw_range_error` is provided by torajs-throw at link
// time (already wired for `construct.rs` and `divmod.rs`); no cfg(test)
// stub needed — the workspace test runner resolves the symbol through
// the existing throw-staticlib dep.

/// Bit length of `|value|` (0 for `0n`).
#[inline]
unsafe fn magnitude_bitlen(p: *const u8) -> u64 {
    unsafe {
        let len = read_len(p) as u64;
        if len == 0 {
            return 0;
        }
        let top = *words_ptr(p).add(len as usize - 1);
        len * 64 - top.leading_zeros() as u64
    }
}

/// Allocate a fresh block of `ceil(bits / 64)` limbs holding
/// `|value| mod 2^bits` (low `bits` bits of the magnitude; limbs past
/// the source are zero, the top limb is masked). Not normalized.
unsafe fn alloc_trunc(value: *const u8, bits: i64) -> *mut u8 {
    unsafe {
        let nl = ((bits + 63) / 64) as u32;
        let b = alloc_raw(nl);
        let src_len = read_len(value);
        let copy = src_len.min(nl) as usize;
        let src = words_ptr(value);
        let dst = words_mut(b);
        for i in 0..copy {
            *dst.add(i) = *src.add(i);
        }
        mask_top_limb(b, bits);
        b
    }
}

/// Mask the top limb of `b` down to `bits mod 64` bits (no-op when
/// `bits` is a limb multiple).
#[inline]
unsafe fn mask_top_limb(b: *mut u8, bits: i64) {
    let rem = (bits % 64) as u32;
    if rem != 0 {
        unsafe {
            let nl = ((bits + 63) / 64) as usize;
            let top = words_mut(b).add(nl - 1);
            *top &= (1u64 << rem) - 1;
        }
    }
}

/// In-place `2^bits - m` over the `bits`-wide window (two's
/// complement). `m == 0` correctly yields 0 (the +1 carry falls off
/// the masked top). Not normalized.
unsafe fn twos_complement_in_place(b: *mut u8, bits: i64) {
    unsafe {
        let nl = ((bits + 63) / 64) as usize;
        let w = words_mut(b);
        let mut carry = 1u64;
        for i in 0..nl {
            let (v, c) = (!*w.add(i)).overflowing_add(carry);
            *w.add(i) = v;
            carry = c as u64;
        }
        mask_top_limb(b, bits);
    }
}

/// Whether bit `bits - 1` (the two's-complement sign bit) is set.
#[inline]
unsafe fn sign_bit_set(b: *const u8, bits: i64) -> bool {
    unsafe {
        let idx = ((bits - 1) / 64) as usize;
        let bit = ((bits - 1) % 64) as u32;
        *words_ptr(b).add(idx) & (1u64 << bit) != 0
    }
}

/// Fresh +1-rc copy of `value` (fast-path result when the input
/// already fits the requested width).
unsafe fn clone_value(value: *const u8) -> *mut u8 {
    unsafe {
        let len = read_len(value);
        let b = alloc_raw(len);
        let src = words_ptr(value);
        let dst = words_mut(b);
        for i in 0..(len as usize) {
            *dst.add(i) = *src.add(i);
        }
        write_sign(b, read_sign(value));
        b
    }
}

/// `BigInt.asUintN(bits, value)` for arbitrary `bits >= 0`. Returns a
/// fresh BigInt heap pointer; caller takes ownership (refcount = 1).
///
/// Negative `bits` (pre-ToIndex callers) and `bits > MAX_BITS` on the
/// negative-input path throw `RangeError` and return a sentinel `0n`.
///
/// # Safety
/// `value` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_as_uint_n(bits: i64, value: *const c_void) -> *mut u8 {
    unsafe {
        let p = value as *const u8;
        if bits < 0 {
            __torajs_throw_range_error(b"BigInt.asUintN: bits must be non-negative\0".as_ptr());
            return alloc_raw(0);
        }
        if bits == 0 {
            return alloc_raw(0);
        }
        let neg = read_sign(p) != 0;
        if !neg && bits as u64 >= magnitude_bitlen(p) {
            // Already < 2^bits: identity.
            return clone_value(p);
        }
        if neg && bits > MAX_BITS {
            // Result genuinely needs `bits` bits (2^bits - low).
            __torajs_throw_range_error(b"Maximum BigInt size exceeded\0".as_ptr());
            return alloc_raw(0);
        }
        let b = alloc_trunc(p, bits);
        if neg {
            twos_complement_in_place(b, bits);
        }
        normalize(b);
        b
    }
}

/// `BigInt.asIntN(bits, value)` for arbitrary `bits >= 0`. Returns a
/// fresh BigInt heap pointer; caller takes ownership (refcount = 1).
///
/// Negative `bits` throws `RangeError` and returns a sentinel `0n`.
/// No size cap needed: the slow path only runs when
/// `bits <= bitlen(|value|)`, so allocation is bounded by the input.
///
/// # Safety
/// `value` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_as_int_n(bits: i64, value: *const c_void) -> *mut u8 {
    unsafe {
        let p = value as *const u8;
        if bits < 0 {
            __torajs_throw_range_error(b"BigInt.asIntN: bits must be non-negative\0".as_ptr());
            return alloc_raw(0);
        }
        if bits == 0 {
            return alloc_raw(0);
        }
        if bits as u64 > magnitude_bitlen(p) {
            // |value| < 2^(bits-1) (and -2^(bits-1) itself falls to the
            // slow path via bits == bitlen): identity.
            return clone_value(p);
        }
        // bits <= bitlen(|value|): allocation bounded by the input.
        let b = alloc_trunc(p, bits);
        if read_sign(p) != 0 {
            twos_complement_in_place(b, bits);
        }
        if sign_bit_set(b, bits) {
            // Negative result: magnitude = 2^bits - u.
            twos_complement_in_place(b, bits);
            write_sign(b, 1);
        }
        normalize(b);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    /// Build a single-limb BigInt for testing. Mirrors how the SSA
    /// layer constructs them from u64 literals.
    unsafe fn make(sign: u32, magnitude: u64) -> *mut u8 {
        unsafe {
            if magnitude == 0 {
                return alloc_raw(0);
            }
            let b = alloc_raw(1);
            *words_mut(b) = magnitude;
            write_sign(b, sign);
            b
        }
    }

    /// Build a multi-limb BigInt from little-endian limbs.
    unsafe fn make_limbs(sign: u32, limbs: &[u64]) -> *mut u8 {
        unsafe {
            let b = alloc_raw(limbs.len() as u32);
            let w = words_mut(b);
            for (i, &l) in limbs.iter().enumerate() {
                *w.add(i) = l;
            }
            write_sign(b, sign);
            normalize(b);
            b
        }
    }

    unsafe fn read_limbs(b: *mut u8) -> (u32, Vec<u64>) {
        unsafe {
            let len = read_len(b) as usize;
            let w = words_ptr(b);
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                out.push(*w.add(i));
            }
            (read_sign(b), out)
        }
    }

    unsafe fn read_back(b: *mut u8) -> (u32, u64) {
        unsafe {
            let (s, l) = read_limbs(b);
            (s, l.first().copied().unwrap_or(0))
        }
    }

    #[test]
    fn as_uint_n_zero_bits() {
        unsafe {
            let v = make(0, 100);
            let out = __torajs_bigint_as_uint_n(0, v as *const c_void);
            assert_eq!(read_back(out), (0, 0));
        }
    }

    #[test]
    fn as_uint_n_positive_in_range() {
        unsafe {
            let v = make(0, 127);
            let out = __torajs_bigint_as_uint_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 127));
        }
    }

    #[test]
    fn as_uint_n_positive_overflow() {
        unsafe {
            // asUintN(8, 256n) = 0n
            let v = make(0, 256);
            let out = __torajs_bigint_as_uint_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 0));
        }
    }

    #[test]
    fn as_uint_n_negative_one() {
        unsafe {
            // asUintN(8, -1n) = 255n
            let v = make(1, 1);
            let out = __torajs_bigint_as_uint_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 255));
        }
    }

    #[test]
    fn as_uint_n_negative_one_64bits() {
        unsafe {
            // asUintN(64, -1n) = 18446744073709551615n
            let v = make(1, 1);
            let out = __torajs_bigint_as_uint_n(64, v as *const c_void);
            assert_eq!(read_back(out), (0, u64::MAX));
        }
    }

    #[test]
    fn as_uint_n_negative_one_128bits() {
        unsafe {
            // asUintN(128, -1n) = 2^128 - 1 (two all-ones limbs)
            let v = make(1, 1);
            let out = __torajs_bigint_as_uint_n(128, v as *const c_void);
            assert_eq!(read_limbs(out), (0, vec![u64::MAX, u64::MAX]));
        }
    }

    #[test]
    fn as_uint_n_negative_one_100bits() {
        unsafe {
            // asUintN(100, -1n) = 2^100 - 1 (top limb masked to 36 bits)
            let v = make(1, 1);
            let out = __torajs_bigint_as_uint_n(100, v as *const c_void);
            assert_eq!(read_limbs(out), (0, vec![u64::MAX, (1u64 << 36) - 1]));
        }
    }

    #[test]
    fn as_uint_n_huge_bits_positive_identity() {
        unsafe {
            // asUintN(2^40, 5n) = 5n — fast path, no huge allocation
            let v = make(0, 5);
            let out = __torajs_bigint_as_uint_n(1i64 << 40, v as *const c_void);
            assert_eq!(read_back(out), (0, 5));
        }
    }

    #[test]
    fn as_uint_n_multi_limb_truncate() {
        unsafe {
            // x = 2^64 + 7, asUintN(64, x) = 7n
            let v = make_limbs(0, &[7, 1]);
            let out = __torajs_bigint_as_uint_n(64, v as *const c_void);
            assert_eq!(read_limbs(out), (0, vec![7]));
        }
    }

    #[test]
    fn as_int_n_in_range() {
        unsafe {
            // asIntN(8, 127n) = 127n
            let v = make(0, 127);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 127));
        }
    }

    #[test]
    fn as_int_n_top_bit_flips_sign() {
        unsafe {
            // asIntN(8, 128n) = -128n
            let v = make(0, 128);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (1, 128));
        }
    }

    #[test]
    fn as_int_n_unsigned_255() {
        unsafe {
            // asIntN(8, 255n) = -1n
            let v = make(0, 255);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (1, 1));
        }
    }

    #[test]
    fn as_int_n_overflow_to_zero() {
        unsafe {
            // asIntN(8, 256n) = 0n
            let v = make(0, 256);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 0));
        }
    }

    #[test]
    fn as_int_n_64bit_min() {
        unsafe {
            // asIntN(64, 2^63 n) = -9223372036854775808n
            let v = make(0, 1u64 << 63);
            let out = __torajs_bigint_as_int_n(64, v as *const c_void);
            assert_eq!(read_back(out), (1, 1u64 << 63));
        }
    }

    #[test]
    fn as_int_n_negative_min_identity() {
        unsafe {
            // asIntN(8, -128n) = -128n (bits == bitlen slow path)
            let v = make(1, 128);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (1, 128));
        }
    }

    #[test]
    fn as_int_n_negative_129_wraps() {
        unsafe {
            // asIntN(8, -129n) = 127n
            let v = make(1, 129);
            let out = __torajs_bigint_as_int_n(8, v as *const c_void);
            assert_eq!(read_back(out), (0, 127));
        }
    }

    #[test]
    fn as_int_n_128bit_min() {
        unsafe {
            // asIntN(128, 2^127) = -2^127
            let v = make_limbs(0, &[0, 1u64 << 63]);
            let out = __torajs_bigint_as_int_n(128, v as *const c_void);
            assert_eq!(read_limbs(out), (1, vec![0, 1u64 << 63]));
        }
    }

    #[test]
    fn as_int_n_multi_limb_positive() {
        unsafe {
            // x = 2^100 + 3, asIntN(100, x) = 3n (mod 2^100, top bit clear)
            let v = make_limbs(0, &[3, 1u64 << 36]);
            let out = __torajs_bigint_as_int_n(100, v as *const c_void);
            assert_eq!(read_limbs(out), (0, vec![3]));
        }
    }

    #[test]
    fn as_int_n_huge_bits_negative_identity() {
        unsafe {
            // asIntN(2^40, -5n) = -5n — fast path, no huge allocation
            let v = make(1, 5);
            let out = __torajs_bigint_as_int_n(1i64 << 40, v as *const c_void);
            assert_eq!(read_back(out), (1, 5));
        }
    }

    // bits < 0 and the asUintN negative-input MAX_BITS cap route
    // through `__torajs_throw_range_error` (panicking stub under
    // cfg(test)); exercised end-to-end via conformance fixtures.
}
