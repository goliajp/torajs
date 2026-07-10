//! `memmove(dst, src, n) -> *mut c_void` — overlap-safe copy.
//!
//! Same signature as `memcpy` but tolerates overlapping
//! ranges. Uses forward iteration when `dst <= src` and
//! reverse iteration when `dst > src` to avoid clobbering
//! source bytes before they're read.

use core::ffi::c_void;

/// `memmove(dst, src, n)` — overlap-safe copy.
///
/// Signature matches the libc prototype exactly (`c_void`
/// pointers) — see `memcpy` for the
/// `suspicious_runtime_symbol_definitions` rationale.
///
/// # Safety
///
/// `dst` writable for ≥ n bytes. `src` readable for ≥ n bytes.
/// Ranges MAY overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dst.cast::<u8>();
    let s = src.cast::<u8>();
    if d as *const u8 == s || n == 0 {
        return dst;
    }
    // Standard overlap-safe pattern: if dst is BELOW src in
    // memory, forward copy is safe (writing destination won't
    // clobber yet-to-read source bytes). If dst is ABOVE src,
    // reverse copy is needed.
    if (d as usize) < (s as usize) {
        let mut i = 0;
        while i < n {
            unsafe {
                *d.add(i) = *s.add(i);
            }
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe {
                *d.add(i) = *s.add(i);
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
        let r = unsafe { memmove(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 0) };
        assert_eq!(r.cast::<u8>(), dst.as_mut_ptr());
        assert_eq!(&dst, &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_overlapping() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { memmove(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 5) };
        assert_eq!(&dst, b"hello");
    }

    /// Forward overlap: `dst` BELOW `src`. Shift "world" left
    /// by 6 (overwriting "hello "). Expected: `world world`.
    #[test]
    fn overlap_forward() {
        let mut buf = [0u8; 11];
        buf.copy_from_slice(b"hello world");
        let p = buf.as_mut_ptr();
        unsafe { memmove(p.cast(), p.add(6).cast(), 5) };
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
        unsafe { memmove(p.add(6).cast(), p.cast(), 5) };
        // Bytes 0..6 retain their original content.
        assert_eq!(&buf[..6], b"hello ");
        assert_eq!(&buf[6..], b"hello");
    }

    #[test]
    fn same_ptr() {
        let mut buf = [1u8, 2, 3];
        let p = buf.as_mut_ptr();
        unsafe { memmove(p.cast(), p.cast(), 3) };
        assert_eq!(buf, [1, 2, 3]);
    }
}
