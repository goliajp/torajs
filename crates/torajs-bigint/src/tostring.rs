//! BigInt → string (any base in [2, 36]).
//!
//! Port of `runtime_bigint.c` lines 305-368 (P3.3-g, 2026-05-23),
//! extended in P12.4 to accept a radix per ES §6.1.6.2.13
//! (`BigInt.prototype.toString(radix)`).
//!
//! Algorithm: successive division by `CHUNK = radix^k`, the largest
//! power of `radix` that fits in a `u64`. Each iteration extracts `k`
//! base-`radix` digits at once. The most-significant chunk emits only
//! as many digits as the remainder needs (no leading zeros); mid-
//! stream chunks always emit exactly `k` digits so joins with
//! downstream chunks don't lose leading zeros. Sign is prepended after.
//!
//! Memory footprint:
//! - A destructively-divided magnitude clone of the input
//! - A write-from-the-tail digit buffer sized at `21 * len + 2` bytes
//!   for radix 10 (largest among bases ≥ 4); radix 2 / 3 need more
//!   headroom and we scale by `chunk_digits + 1` accordingly.
//! - Final Str allocation via [`crate::str_bridge::alloc_str`]
//!
//! All freed before return.

use core::ffi::c_void;

use crate::internal::{alloc_raw, free, normalize, read_len, read_sign, words_mut, words_ptr};
use crate::str_bridge::alloc_str;

/// 10^19 — largest power of ten that fits in a `u64`. Kept as a
/// pre-computed constant for the decimal-fast-path.
const DEC_CHUNK: u64 = 10_000_000_000_000_000_000u64;
const DEC_CHUNK_DIGITS: usize = 19;

/// Compute `(chunk, k)` such that `chunk = radix^k` is the largest
/// power of `radix` that fits in a `u64`. Caller must ensure
/// `radix >= 2` (so the loop terminates).
fn radix_chunk(radix: u32) -> (u64, usize) {
    debug_assert!((2..=36).contains(&radix));
    let mut chunk: u64 = 1;
    let mut k: usize = 0;
    loop {
        match chunk.checked_mul(radix as u64) {
            Some(n) => {
                chunk = n;
                k += 1;
            }
            None => return (chunk, k),
        }
    }
}

/// Map a digit value in `[0, 35]` to its ASCII character per the
/// canonical lowercase JS / Python convention.
#[inline]
fn digit_char(d: u8) -> u8 {
    debug_assert!(d < 36);
    if d < 10 { b'0' + d } else { b'a' + (d - 10) }
}

/// Divide magnitude in place by `chunk` (a `u64`). Returns the
/// remainder. Normalizes the magnitude on exit.
///
/// Walks limbs high → low; at each step combines previous remainder
/// (shifted into the high 64 bits) with the current limb (low 64) as a
/// `u128` dividend, divides by `chunk`, stores quotient back, carries
/// the remainder forward.
unsafe fn divmod_chunk(b: *mut u8, chunk: u64) -> u64 {
    unsafe {
        let len = read_len(b) as usize;
        let w = words_mut(b);
        let mut rem: u64 = 0;
        // Walk high → low.
        let mut i = len as isize - 1;
        while i >= 0 {
            let cur: u128 = ((rem as u128) << 64) | (*w.add(i as usize) as u128);
            *w.add(i as usize) = (cur / chunk as u128) as u64;
            rem = (cur % chunk as u128) as u64;
            i -= 1;
        }
        normalize(b);
        rem
    }
}

/// Encode `a` as a base-`radix` digit string with sign prefix.
///
/// `radix` must be in `[2, 36]`; the caller is responsible for the
/// invariant. For decimal output, the existing public extern
/// (`__torajs_bigint_to_string`) calls in with `radix = 10` and
/// inherits the original tight loop via the pre-baked `DEC_CHUNK`.
///
/// # Safety
/// `a_` must be a valid BigInt heap pointer.
unsafe fn to_string_radix(a_: *const c_void, radix: u32) -> *mut u8 {
    let a = a_ as *const u8;
    unsafe {
        let a_len = read_len(a);
        if a_len == 0 {
            return alloc_str(b"0");
        }
        let (chunk, chunk_digits) = if radix == 10 {
            (DEC_CHUNK, DEC_CHUNK_DIGITS)
        } else {
            radix_chunk(radix)
        };
        // Clone magnitude so we can destructively divide.
        let tmp = alloc_raw(a_len);
        {
            let src = words_ptr(a);
            let dst = words_mut(tmp);
            for i in 0..(a_len as usize) {
                *dst.add(i) = *src.add(i);
            }
        }
        // Worst-case digit count per u64 limb scales with `chunk_digits +
        // 1`. Radix 2 hits 64 / chunk_digits ≈ 64 / 63 ≈ 1.02 — but each
        // limb produces 64 base-2 digits in the worst case. Bound at
        // `(64 / chunk_digits + 1) * chunk_digits * len + 2`, which
        // simplifies to `(chunk_digits + 64) * len + 2` (loose, safe).
        let cap = (chunk_digits + 64) * (a_len as usize) + 2;
        let mut buf = vec![0u8; cap];
        let mut pos = cap;
        while read_len(tmp) > 0 {
            let mut rem = divmod_chunk(tmp, chunk);
            let more_chunks_remain = read_len(tmp) > 0;
            if !more_chunks_remain {
                // Most-significant chunk: emit only as many digits as
                // the remainder needs (no leading zeros).
                loop {
                    pos -= 1;
                    buf[pos] = digit_char((rem % radix as u64) as u8);
                    rem /= radix as u64;
                    if rem == 0 {
                        break;
                    }
                }
            } else {
                // Mid-stream chunk: always emit exactly `chunk_digits`
                // digits so joins with downstream chunks don't lose
                // leading zeros.
                for _ in 0..chunk_digits {
                    pos -= 1;
                    buf[pos] = digit_char((rem % radix as u64) as u8);
                    rem /= radix as u64;
                }
            }
        }
        free(tmp as *mut c_void);
        if read_sign(a) != 0 {
            pos -= 1;
            buf[pos] = b'-';
        }
        alloc_str(&buf[pos..])
    }
}

/// `a.toString()` for BigInt. Sign-aware decimal encoding.
///
/// # Safety
/// `a_` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_to_string(a_: *const c_void) -> *mut u8 {
    unsafe { to_string_radix(a_, 10) }
}

/// `a.toString(radix)` for BigInt per ES §6.1.6.2.13. `radix` is
/// validated by the caller to be in `[2, 36]`; out-of-range inputs
/// must throw `RangeError` at the call site (the SSA-emit layer is
/// responsible — this extern's contract is "given a valid radix").
///
/// # Safety
/// `a_` must be a valid BigInt heap pointer; `radix` must be in
/// `[2, 36]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_to_string_radix(a_: *const c_void, radix: i64) -> *mut u8 {
    debug_assert!((2..=36).contains(&radix));
    unsafe { to_string_radix(a_, radix as u32) }
}
