//! `memcpy(dst, src, n) -> *mut c_void` — forward byte copy.
//!
//! LLVM expects `memcpy` to assume non-overlapping `dst` /
//! `src`. Caller is responsible for ensuring no overlap; for
//! overlap-safe copy, use [`memmove`](super::memmove::memmove).

use core::ffi::c_void;

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
    // Forward byte loop. LLVM auto-vectorizes to NEON ldp/stp
    // on aarch64 + SSE/AVX2 on x86_64 for n ≥ 16. For small n
    // (< 16) the unrolled bytes stay in scalar regs.
    let mut i = 0;
    while i < n {
        unsafe {
            *d.add(i) = *s.add(i);
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
}
