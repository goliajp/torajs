//! The 64-bit window a typed array sees a BigInt through — the two
//! halves of §7.1.15 `ToBigInt64` / §7.1.16 `ToBigUint64`
//! (RFC 20260823-typedarray-substrate 刀 2).
//!
//! Both directions are the SAME 64 bits: `BigInt64Array` and
//! `BigUint64Array` differ only in how the element is read back, not
//! in what a write stores. So the write side answers a `u64` and the
//! read side takes one — the sign lives in the element type, not
//! here.

use core::ffi::c_void;

use crate::internal::{alloc_raw, read_len, read_sign, words_mut, words_ptr, write_sign};

/// `BigInt(<u64>)` — the unsigned mint `from_i64` cannot express
/// above `i64::MAX`, which is exactly the half of `BigUint64Array`'s
/// range that matters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_u64(v: u64) -> *mut u8 {
    if v == 0 {
        return unsafe { alloc_raw(0) };
    }
    let b = unsafe { alloc_raw(1) };
    unsafe {
        write_sign(b, 0);
        *words_mut(b) = v;
    }
    b
}

/// The low 64 bits of a BigInt's two's-complement value.
///
/// Magnitude limbs above the first are dropped (that IS the modulo),
/// and a negative value is negated in the 64-bit ring — so a
/// `-1n` write stores `0xFFFF_FFFF_FFFF_FFFF`, which `BigInt64Array`
/// reads back as `-1n` and `BigUint64Array` as `2n**64n - 1n`.
///
/// # Safety
/// `p` is a live BigInt cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_to_u64_wrapping(p: *const c_void) -> u64 {
    let a = p as *const u8;
    let len = unsafe { read_len(a) };
    if len == 0 {
        return 0;
    }
    let low = unsafe { *words_ptr(a) };
    if unsafe { read_sign(a) } == 0 {
        low
    } else {
        low.wrapping_neg()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn round_trip(v: u64) -> u64 {
        unsafe {
            let cell = __torajs_bigint_from_u64(v);
            let out = __torajs_bigint_to_u64_wrapping(cell as *const c_void);
            crate::__torajs_bigint_drop(cell as *mut c_void);
            out
        }
    }

    #[test]
    fn unsigned_values_survive_the_round_trip() {
        for v in [0u64, 1, 255, i64::MAX as u64, u64::MAX, u64::MAX / 3] {
            assert_eq!(unsafe { round_trip(v) }, v, "value {v}");
        }
    }

    #[test]
    fn a_negative_bigint_is_its_twos_complement() {
        unsafe {
            let minus_one = crate::__torajs_bigint_from_i64(-1);
            assert_eq!(
                __torajs_bigint_to_u64_wrapping(minus_one as *const c_void),
                u64::MAX
            );
            crate::__torajs_bigint_drop(minus_one as *mut c_void);

            let min = crate::__torajs_bigint_from_i64(i64::MIN);
            assert_eq!(
                __torajs_bigint_to_u64_wrapping(min as *const c_void),
                1u64 << 63
            );
            crate::__torajs_bigint_drop(min as *mut c_void);
        }
    }
}
