//! `memcpy(dst, src, n) -> *mut c_void` — forward copy, 16 bytes at a
//! time (64 per iteration once there is that much left).
//!
//! LLVM expects `memcpy` to assume non-overlapping `dst` /
//! `src`. Caller is responsible for ensuring no overlap; for
//! overlap-safe copy, use [`memmove`](super::memmove::memmove).

use core::ffi::c_void;

use crate::word::{cp_small, cp16};

/// `memcpy(dst, src, n)` — non-overlapping forward copy.
///
/// Signature matches the libc prototype exactly (`c_void`
/// pointers) — rustc's `suspicious_runtime_symbol_definitions`
/// lint (new in nightly 1.99) flags any other spelling of a
/// symbol std itself calls.
///
/// # Safety
///
/// `dst` writable for ≥ n bytes. `src` readable for ≥ n bytes.
/// `dst` and `src` MUST NOT overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dst.cast::<u8>();
    let s = src.cast::<u8>();
    unsafe {
        if n < 16 {
            cp_small(d, s, n);
            return dst;
        }
        let mut i = 0;
        while n - i >= 64 {
            cp16(d.add(i), s.add(i));
            cp16(d.add(i + 16), s.add(i + 16));
            cp16(d.add(i + 32), s.add(i + 32));
            cp16(d.add(i + 48), s.add(i + 48));
            i += 64;
        }
        while n - i >= 16 {
            cp16(d.add(i), s.add(i));
            i += 16;
        }
        // the last partial block, as the 16 bytes ending at `n`: the
        // ranges do not overlap, so recopying some bytes is harmless
        if i < n {
            cp16(d.add(n - 16), s.add(n - 16));
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_n_is_noop() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        let r = unsafe { memcpy(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 0) };
        assert_eq!(r.cast::<u8>(), dst.as_mut_ptr());
        assert_eq!(&dst, &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn basic() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { memcpy(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 5) };
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn long() {
        let src: [u8; 64] = core::array::from_fn(|i| i as u8);
        let mut dst = [0u8; 64];
        unsafe { memcpy(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 64) };
        assert_eq!(&dst, &src);
    }

    #[test]
    fn partial() {
        let src = b"hello world";
        let mut dst = [b'.'; 11];
        unsafe { memcpy(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 5) };
        assert_eq!(&dst, b"hello......");
    }

    /// Every length across the small / 16-block / 64-block seams, at
    /// every source and destination alignment, with guard bytes on
    /// both sides of the destination.
    #[test]
    fn every_length_and_alignment() {
        let src: [u8; 400] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        for n in 0..=300usize {
            for (so, d_off) in [(0usize, 0usize), (1, 3), (5, 2), (7, 7)] {
                let mut dst = [0xEEu8; 400];
                unsafe {
                    memcpy(
                        dst.as_mut_ptr().add(8 + d_off).cast(),
                        src.as_ptr().add(so).cast(),
                        n,
                    )
                };
                assert_eq!(&dst[8 + d_off..8 + d_off + n], &src[so..so + n], "n={n}");
                assert!(dst[..8 + d_off].iter().all(|&b| b == 0xEE), "n={n} head");
                assert!(
                    dst[8 + d_off + n..].iter().all(|&b| b == 0xEE),
                    "n={n} tail"
                );
            }
        }
    }
}
