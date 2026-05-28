//! i64 → decimal string. v0.7-A4 Step 15-d follow-on:
//! provides a 0-libc i64 formatter alongside dtoa so the
//! print + join + i64_to_str call sites don't need to reach
//! for libc `snprintf("%lld")`.
//!
//! Implementation uses Rust's `core::fmt::Write` into a
//! caller-provided byte slice — same `ByteWriter`-style
//! pattern as `dtoa.rs`. i64's decimal representation is at
//! most 20 bytes (`-9223372036854775808`); caller buffers
//! ≥ 24 bytes cover every case with headroom.

use core::fmt::Write;

struct ByteWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Write for ByteWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len();
        let remaining = self.buf.len().saturating_sub(self.pos);
        let copy_len = n.min(remaining);
        if copy_len > 0 {
            self.buf[self.pos..self.pos + copy_len].copy_from_slice(&bytes[..copy_len]);
            self.pos += copy_len;
        }
        if copy_len < n {
            Err(core::fmt::Error)
        } else {
            Ok(())
        }
    }
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
    if out_buf.is_null() || out_cap < 24 {
        return -1;
    }
    // SAFETY: caller covenant — out_buf writable for out_cap bytes.
    let buf = unsafe { core::slice::from_raw_parts_mut(out_buf, out_cap) };
    let mut writer = ByteWriter { buf, pos: 0 };
    if write!(writer, "{}", n).is_err() {
        return -1;
    }
    writer.pos as i32
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
}
