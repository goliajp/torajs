//! `memmove(dst, src, n) -> *mut u8` — overlap-safe copy.
//!
//! Same signature as `memcpy` but tolerates overlapping
//! ranges. Uses forward iteration when `dst <= src` and
//! reverse iteration when `dst > src` to avoid clobbering
//! source bytes before they're read.

/// `memmove(dst, src, n)` — overlap-safe copy.
///
/// # Safety
///
/// `dst` writable for ≥ n bytes. `src` readable for ≥ n bytes.
/// Ranges MAY overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if dst as *const u8 == src || n == 0 {
        return dst;
    }
    // Standard overlap-safe pattern: if dst is BELOW src in
    // memory, forward copy is safe (writing destination won't
    // clobber yet-to-read source bytes). If dst is ABOVE src,
    // reverse copy is needed.
    if (dst as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            unsafe {
                *dst.add(i) = *src.add(i);
            }
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe {
                *dst.add(i) = *src.add(i);
            }
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
        let r = unsafe { memmove(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(r, dst.as_mut_ptr());
        assert_eq!(&dst, &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_overlapping() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { memmove(dst.as_mut_ptr(), src.as_ptr(), 5) };
        assert_eq!(&dst, b"hello");
    }

    /// Forward overlap: `dst` BELOW `src`. Shift "world" left
    /// by 6 (overwriting "hello "). Expected: `world world`.
    #[test]
    fn overlap_forward() {
        let mut buf = [0u8; 11];
        buf.copy_from_slice(b"hello world");
        let p = buf.as_mut_ptr();
        unsafe { memmove(p, p.add(6), 5) };
        assert_eq!(&buf[..5], b"world");
        // Bytes 5..11 retain their original content.
        assert_eq!(&buf[5..], b" world");
    }

    /// Backward overlap: `dst` ABOVE `src`. Shift "hello" right
    /// by 6. Expected: bytes 6..11 = "hello".
    #[test]
    fn overlap_backward() {
        let mut buf = [0u8; 11];
        buf.copy_from_slice(b"hello world");
        let p = buf.as_mut_ptr();
        unsafe { memmove(p.add(6), p, 5) };
        // Bytes 0..6 retain their original content.
        assert_eq!(&buf[..6], b"hello ");
        assert_eq!(&buf[6..], b"hello");
    }

    #[test]
    fn same_ptr() {
        let mut buf = [1u8, 2, 3];
        let p = buf.as_mut_ptr();
        unsafe { memmove(p, p, 3) };
        assert_eq!(buf, [1, 2, 3]);
    }
}
