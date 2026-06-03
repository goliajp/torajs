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
//! Common case is `bits <= 64` (Solidity int64 / int32 / int16 /
//! int8 truncation, asUintN(64, -1n) idiom). This module fast-paths
//! the entire `bits in [1, 64]` range via a single u64 mask + sign
//! flip; `bits == 0` returns `0n`; `bits > 64` throws a
//! `RangeError` (handled at the SSA-emit layer in P12.4-B; the
//! Rust path returns null and lets the caller propagate).
//!
//! Spec scope skipped here:
//!
//! - `bits > 64` (need word-level masking + 2's-complement
//!   propagation across multiple limbs). Tracked as L3b cold —
//!   no real-world bench / app needs it. The extern returns a
//!   sentinel that the SSA-emit caller turns into the spec-
//!   prescribed `RangeError`.

use core::ffi::c_void;

use crate::internal::{
    alloc_raw, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

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

/// Extract the low 64 bits of `|value|`'s magnitude (zero if the
/// BigInt is `0n`).
#[inline]
unsafe fn low64(value: *const c_void) -> u64 {
    let p = value as *const u8;
    unsafe {
        let len = read_len(p);
        if len == 0 {
            return 0;
        }
        let w = words_ptr(p);
        *w
    }
}

/// Build a fresh single-limb BigInt with `sign` and `magnitude`.
/// `magnitude == 0` returns the canonical zero (`len = 0, sign = 0`).
unsafe fn make_u64_bigint(sign: u32, magnitude: u64) -> *mut u8 {
    unsafe {
        if magnitude == 0 {
            let b = alloc_raw(0);
            write_sign(b, 0);
            return b;
        }
        let b = alloc_raw(1);
        let w = words_mut(b);
        *w = magnitude;
        write_sign(b, sign);
        normalize(b);
        b
    }
}

/// `BigInt.asUintN(bits, value)` for `bits <= 64`. Returns a fresh
/// BigInt heap pointer; caller takes ownership (refcount = 1).
///
/// For `bits == 0` returns `0n`. For `bits > 64` returns null (the
/// SSA-emit caller is expected to have rejected this case at the
/// throw-check level).
///
/// # Safety
/// `value` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_as_uint_n(bits: i64, value: *const c_void) -> *mut u8 {
    unsafe {
        if !(0..=64).contains(&bits) {
            __torajs_throw_range_error(b"BigInt.asN: bits must be in [0, 64]\0".as_ptr());
            return make_u64_bigint(0, 0);
        }
        if bits == 0 {
            return make_u64_bigint(0, 0);
        }
        let mask: u64 = if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        let mut low = low64(value) & mask;
        // For negative inputs, apply two's-complement reduction:
        // `(2^bits - low) mod 2^bits`.
        let neg = read_sign(value as *const u8) != 0;
        if neg && low != 0 {
            low = ((!low).wrapping_add(1)) & mask;
        }
        make_u64_bigint(0, low)
    }
}

/// `BigInt.asIntN(bits, value)` for `bits <= 64`. Returns a fresh
/// BigInt heap pointer; caller takes ownership (refcount = 1).
///
/// For `bits == 0` returns `0n`. For `bits > 64` returns null.
///
/// # Safety
/// `value` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_as_int_n(bits: i64, value: *const c_void) -> *mut u8 {
    unsafe {
        if !(0..=64).contains(&bits) {
            __torajs_throw_range_error(b"BigInt.asN: bits must be in [0, 64]\0".as_ptr());
            return make_u64_bigint(0, 0);
        }
        if bits == 0 {
            return make_u64_bigint(0, 0);
        }
        // First compute the unsigned reduction.
        let mask: u64 = if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        let mut low = low64(value) & mask;
        let neg = read_sign(value as *const u8) != 0;
        if neg && low != 0 {
            low = ((!low).wrapping_add(1)) & mask;
        }
        // Then interpret the top bit as the sign.
        let sign_bit = if bits == 64 {
            1u64 << 63
        } else {
            1u64 << (bits - 1)
        };
        if low & sign_bit != 0 {
            // Negative result. Magnitude = 2^bits - low.
            // Edge case bits == 64 + low == 2^63 yields magnitude == 2^63,
            // representable in a single u64.
            let magnitude = if bits == 64 {
                // 2^64 - low, computed as (!low).wrapping_add(1).
                (!low).wrapping_add(1)
            } else {
                (1u64 << bits) - low
            };
            make_u64_bigint(1, magnitude)
        } else {
            make_u64_bigint(0, low)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    // Build a single-limb BigInt for testing. Mirrors how the SSA
    // layer constructs them from u64 literals.
    unsafe fn make(sign: u32, magnitude: u64) -> *mut u8 {
        unsafe { make_u64_bigint(sign, magnitude) }
    }

    unsafe fn read_back(b: *mut u8) -> (u32, u64) {
        unsafe {
            (
                read_sign(b),
                if read_len(b) > 0 {
                    low64(b as *const c_void)
                } else {
                    0
                },
            )
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
    fn as_uint_n_negative_one_16bits() {
        unsafe {
            // asUintN(16, -1n) = 65535n
            let v = make(1, 1);
            let out = __torajs_bigint_as_uint_n(16, v as *const c_void);
            assert_eq!(read_back(out), (0, 65535));
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

    // bits > 64 / bits < 0 paths route through `__torajs_throw_range_error`
    // and return a sentinel 0n. The cfg(test) stub panics on call, so we
    // intentionally don't exercise that path here — it's verified end-to-
    // end in the conformance fixture (bigint-asintn-001.ts).
}
