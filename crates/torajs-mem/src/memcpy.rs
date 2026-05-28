//! `memcpy(dst, src, n) -> *mut u8` — forward byte copy.
//!
//! LLVM expects `memcpy` to assume non-overlapping `dst` /
//! `src`. Caller is responsible for ensuring no overlap; for
//! overlap-safe copy, use [`memmove`](super::memmove::memmove).

/// `memcpy(dst, src, n)` — non-overlapping forward copy.
///
/// # Safety
///
/// `dst` writable for ≥ n bytes. `src` readable for ≥ n bytes.
/// `dst` and `src` MUST NOT overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // Forward byte loop. LLVM auto-vectorizes to NEON ldp/stp
    // on aarch64 + SSE/AVX2 on x86_64 for n ≥ 16. For small n
    // (< 16) the unrolled bytes stay in scalar regs.
    let mut i = 0;
    while i < n {
        unsafe {
            *dst.add(i) = *src.add(i);
        }
        i += 1;
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
        let r = unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(r, dst.as_mut_ptr());
        assert_eq!(&dst, &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn basic() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 5) };
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn long() {
        let src: [u8; 64] = core::array::from_fn(|i| i as u8);
        let mut dst = [0u8; 64];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 64) };
        assert_eq!(&dst, &src);
    }

    #[test]
    fn partial() {
        let src = b"hello world";
        let mut dst = [b'.'; 11];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 5) };
        assert_eq!(&dst, b"hello......");
    }
}
