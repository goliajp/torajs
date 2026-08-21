//! i64 → decimal string. v0.7-A4 Step 15-d follow-on:
//! provides a 0-libc i64 formatter alongside dtoa so the
//! print + join + i64_to_str call sites don't need to reach
//! for libc `snprintf("%lld")`.
//!
//! The digits come out two at a time off a 200-byte pair table —
//! the textbook shape, and the same one `core::fmt`'s integer impl
//! uses underneath. What this module exists to skip is everything
//! `core::fmt` wraps around it: `Formatter`, `pad_integral`, the
//! `Display` dispatch, and a `write_str` call per fragment. A
//! leaf-symbol profile of `JSON.stringify` in a loop (rotation 466)
//! put that machinery at 16% of self-time, against a cost table that
//! prices `format!` of a 6-digit int at 50-100 ns and a stack itoa
//! at 5-10.
//!
//! [`itoa_into`] is the Rust-facing entry (no FFI, no allocation);
//! [`__torajs_fmt_itoa`] is the `extern "C"` shell over it.

/// `"00".."99"`, two bytes per entry. Indexing is `value * 2`.
const DIGIT_PAIRS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";

/// Longest i64 rendering is `-9223372036854775808` — 20 bytes.
pub const ITOA_BUF_LEN: usize = 24;

/// Write `n`'s decimal form into the tail of `buf` and return the
/// index it starts at; the caller reads `&buf[start..]`.
///
/// Filling backwards is what makes the two-at-a-time loop work
/// without a second reversing pass: each step peels the low two
/// digits, which are the ones that belong furthest right.
///
/// `i64::MIN` has no positive counterpart, so the magnitude is taken
/// in `u64` via `unsigned_abs` rather than by negating.
#[inline]
pub fn itoa_into(n: i64, buf: &mut [u8; ITOA_BUF_LEN]) -> usize {
    let negative = n < 0;
    let mut v = n.unsigned_abs();
    let mut pos = ITOA_BUF_LEN;

    while v >= 100 {
        let idx = ((v % 100) as usize) * 2;
        v /= 100;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[idx];
        buf[pos + 1] = DIGIT_PAIRS[idx + 1];
    }
    if v >= 10 {
        let idx = (v as usize) * 2;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[idx];
        buf[pos + 1] = DIGIT_PAIRS[idx + 1];
    } else {
        pos -= 1;
        buf[pos] = b'0' + v as u8;
    }
    if negative {
        pos -= 1;
        buf[pos] = b'-';
    }
    pos
}

/// `__torajs_fmt_itoa(n, out_buf, out_cap) -> bytes_written | -1`.
///
/// Writes the decimal representation of `n` to `out_buf`.
/// Returns the number of bytes written, or -1 on out-of-bounds
/// / buffer too small (< 24 bytes).
///
/// # Safety
///
/// `out_buf` must be writable for at least `out_cap` bytes.
/// `out_cap` ≥ 24 covers every valid i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32 {
    if out_buf.is_null() || out_cap < ITOA_BUF_LEN {
        return -1;
    }
    let mut scratch = [0u8; ITOA_BUF_LEN];
    let start = itoa_into(n, &mut scratch);
    let len = ITOA_BUF_LEN - start;
    // SAFETY: caller covenant — out_buf writable for out_cap bytes,
    // and out_cap ≥ ITOA_BUF_LEN ≥ len was checked above.
    unsafe { core::ptr::copy_nonoverlapping(scratch.as_ptr().add(start), out_buf, len) };
    len as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;
    use alloc::string::{String, ToString as _};

    fn call(n: i64) -> String {
        let mut buf = [0u8; 24];
        let len = unsafe { __torajs_fmt_itoa(n, buf.as_mut_ptr(), buf.len()) };
        assert!(len > 0, "itoa returned {len} for {n}");
        core::str::from_utf8(&buf[..len as usize])
            .unwrap()
            .to_string()
    }

    #[test]
    fn zero() {
        assert_eq!(call(0).as_str(), "0");
    }

    #[test]
    fn positive() {
        assert_eq!(call(42).as_str(), "42");
    }

    #[test]
    fn negative() {
        assert_eq!(call(-7).as_str(), "-7");
    }

    #[test]
    fn max() {
        assert_eq!(call(i64::MAX).as_str(), "9223372036854775807");
    }

    #[test]
    fn min() {
        // i64::MIN's representation is "-9223372036854775808" (20 chars).
        assert_eq!(call(i64::MIN).as_str(), "-9223372036854775808");
    }

    #[test]
    fn buffer_too_small() {
        let mut buf = [0u8; 8];
        let n = unsafe { __torajs_fmt_itoa(42, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(n, -1);
    }

    /// The pair table and the odd/even split are where an itoa
    /// usually goes wrong, so walk the boundaries where the loop
    /// changes shape rather than spot-checking.
    #[test]
    fn digit_count_boundaries() {
        for p in 0..19u32 {
            let base = 10i64.pow(p);
            for n in [base - 1, base, base + 1] {
                assert_eq!(call(n), n.to_string(), "n = {n}");
                assert_eq!(call(-n), (-n).to_string(), "n = {}", -n);
            }
        }
    }

    /// Every value core::fmt would render, rendered the same way.
    #[test]
    fn agrees_with_core_fmt_over_a_sweep() {
        let mut n: i64 = 1;
        for _ in 0..4000 {
            assert_eq!(call(n), n.to_string());
            assert_eq!(call(-n), (-n).to_string());
            // 6364136223846793005 is the LCG multiplier from
            // Numerical Recipes — a cheap way to walk the range
            // without a dependency.
            n = n
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            if n == i64::MIN {
                n = 1;
            }
        }
    }
}
