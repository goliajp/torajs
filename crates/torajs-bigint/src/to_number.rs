//! §21.1.1.1 step 3 — `Number(bigint)`: the one legal BigInt→Number
//! conversion face. `Number(value)` runs ToNumeric and, when the
//! primitive is a BigInt, answers 𝔽(ℝ(value)) — the mathematical
//! value rounded to f64 (round-to-nearest, ties-to-even, per
//! §6.1.6.1). Generic ToNumber(BigInt) keeps throwing (§7.1.4
//! step 2); callers with the legal path dispatch the tag BEFORE the
//! generic kernel, exactly like the arith/compare families.

use crate::layout::{LEN_OFF, SIGN_OFF, WORDS_OFF};

/// 2^64 as f64 — a power of two, so every scaling multiply below is
/// exact (exponent-only) until it overflows to ±Infinity, which is
/// the §6.1.6.1 ℝ→𝔽 answer for magnitudes past the f64 range.
const TWO_POW_64: f64 = 18446744073709551616.0;

/// The f64 value of a BigInt cell (borrowed — no rc traffic).
///
/// Rounding: a ≤1-limb magnitude rides Rust's u64→f64 cast
/// (round-to-nearest-even). A multi-limb magnitude reduces to one
/// correctly rounded u128→f64 conversion: the top two limbs carry
/// ≥ 64 significant bits (top limb non-zero per the canonical
/// invariant) — past the 53-bit mantissa plus its guard/round
/// positions — and OR-ing "any lower limb non-zero" into bit 0
/// (the classic sticky reduction) makes that single rounding decide
/// exactly as the full value would. The remaining 2^64 scaling steps
/// only move the exponent.
///
/// # Safety
/// `b` points at a live canonical BigInt cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_to_number(b: *const u8) -> f64 {
    let sign = unsafe { (b.add(SIGN_OFF) as *const u32).read() };
    let len = unsafe { (b.add(LEN_OFF) as *const u32).read() } as usize;
    if len == 0 {
        return 0.0;
    }
    // SAFETY: canonical cell carries `len` limbs at WORDS_OFF.
    let words = unsafe { core::slice::from_raw_parts(b.add(WORDS_OFF) as *const u64, len) };
    let mag = if len == 1 {
        words[0] as f64
    } else {
        let hi = words[len - 1] as u128;
        let lo = words[len - 2] as u128;
        let mut top = (hi << 64) | lo;
        if words[..len - 2].iter().any(|&w| w != 0) {
            top |= 1;
        }
        let mut v = top as f64;
        for _ in 0..len - 2 {
            v *= TWO_POW_64;
        }
        v
    };
    if sign != 0 { -mag } else { mag }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::{alloc_raw, words_mut, write_sign};

    fn to_num(words: &[u64], sign: u32) -> f64 {
        unsafe {
            let b = alloc_raw(words.len() as u32);
            write_sign(b, sign);
            for (i, w) in words.iter().enumerate() {
                *words_mut(b).add(i) = *w;
            }
            let n = __torajs_bigint_to_number(b);
            crate::drop::__torajs_bigint_drop(b as *mut core::ffi::c_void);
            n
        }
    }

    #[test]
    fn zero_and_small() {
        assert_eq!(to_num(&[], 0), 0.0);
        assert_eq!(to_num(&[10], 0), 10.0);
        assert_eq!(to_num(&[3], 1), -3.0);
    }

    #[test]
    fn two_pow_64() {
        assert_eq!(to_num(&[0, 1], 0), TWO_POW_64);
    }

    #[test]
    fn sticky_breaks_tie() {
        // 2^64 + 2^11: the ulp at this magnitude is 2^12, so 2^11 is
        // exactly half — ties-to-even rounds down to 2^64; a single
        // set bit anywhere below pushes it up to 2^64 + 2^12.
        assert_eq!(to_num(&[1u64 << 11, 1], 0), TWO_POW_64);
        assert_eq!(to_num(&[(1u64 << 11) | 1, 1], 0), TWO_POW_64 + 4096.0);
        // The deciding bit in a THIRD limb — only the sticky fold
        // sees it: 2^128 + 2^75 ties down to 2^128; the +1 far below
        // rounds it up to 2^128 + 2^76.
        let p128 = (2f64).powi(128);
        assert_eq!(to_num(&[0, 1u64 << 11, 1], 0), p128);
        assert_eq!(to_num(&[1, 1u64 << 11, 1], 0), p128 + (2f64).powi(76));
    }

    #[test]
    fn huge_overflows_to_infinity() {
        // 2^1088 — past the f64 max exponent.
        let mut words = vec![0u64; 18];
        words[17] = 1;
        assert_eq!(to_num(&words, 0), f64::INFINITY);
        assert_eq!(to_num(&words, 1), f64::NEG_INFINITY);
    }
}
