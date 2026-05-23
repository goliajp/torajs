//! BigInt → decimal string.
//!
//! Port of `runtime_bigint.c` lines 305-368 (P3.3-g, 2026-05-23).
//!
//! Algorithm: successive division by `DEC_CHUNK = 10^19` (the largest
//! power of ten that fits in a `u64`). Each chunk emits up to 19 ASCII
//! decimal digits; the most-significant chunk emits only as many digits
//! as the remainder needs (no leading zeros). Sign is prepended after.
//!
//! Memory footprint:
//! - A destructively-divided magnitude clone of the input
//! - A write-from-the-tail digit buffer sized at `21 * len + 2` bytes
//! - Final Str allocation via [`crate::str_bridge::alloc_str`]
//!
//! All freed before return.

use core::ffi::c_void;

use crate::internal::{alloc_raw, free, normalize, read_len, read_sign, words_mut, words_ptr};
use crate::str_bridge::alloc_str;

/// 10^19 — largest power of ten that fits in a `u64`. Picking the
/// largest power keeps the divmod-by-chunk loop tight (each iteration
/// extracts 19 digits at once).
const DEC_CHUNK: u64 = 10_000_000_000_000_000_000u64;

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

/// `a.toString()` for BigInt. Sign-aware decimal encoding.
///
/// # Safety
/// `a_` must be a valid BigInt heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_to_string(a_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    unsafe {
        let a_len = read_len(a);
        if a_len == 0 {
            return alloc_str(b"0");
        }
        // Clone magnitude so we can destructively divide.
        let tmp = alloc_raw(a_len);
        {
            let src = words_ptr(a);
            let dst = words_mut(tmp);
            for i in 0..(a_len as usize) {
                *dst.add(i) = *src.add(i);
            }
        }
        // Each u64 limb produces up to ~20 decimal digits; bound output
        // buffer at 21 * len + 2 (sign + headroom). Use a Vec<u8> so we
        // don't manage raw malloc; Rust's Vec frees automatically.
        let cap = (a_len as usize) * 21 + 2;
        let mut buf = vec![0u8; cap];
        let mut pos = cap;
        while read_len(tmp) > 0 {
            let mut rem = divmod_chunk(tmp, DEC_CHUNK);
            let more_chunks_remain = read_len(tmp) > 0;
            if !more_chunks_remain {
                // Most-significant chunk: emit only as many digits as
                // the remainder needs (no leading zeros).
                loop {
                    pos -= 1;
                    buf[pos] = b'0' + (rem % 10) as u8;
                    rem /= 10;
                    if rem == 0 {
                        break;
                    }
                }
            } else {
                // Mid-stream chunk: always emit exactly 19 digits so
                // joins with downstream chunks don't lose leading zeros.
                for _ in 0..19 {
                    pos -= 1;
                    buf[pos] = b'0' + (rem % 10) as u8;
                    rem /= 10;
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
