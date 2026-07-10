//! `memcmp(a, b, n) -> i32` — byte-by-byte comparison.
//!
//! Returns 0 if equal; <0 if first differing byte in `a` is
//! less than `b`; >0 if greater. Matches libc memcmp's
//! "first differing byte difference" semantics.

use core::ffi::c_void;

/// `memcmp(a, b, n) -> i32`.
///
/// Signature matches the libc prototype exactly (`c_void`
/// pointers) — see `memcpy` for the
/// `suspicious_runtime_symbol_definitions` rationale.
///
/// # Safety
///
/// `a` readable for ≥ n bytes. `b` readable for ≥ n bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let pa = a.cast::<u8>();
    let pb = b.cast::<u8>();
    let mut i = 0;
    while i < n {
        // SAFETY: caller covenant — a, b readable for n bytes.
        let av = unsafe { *pa.add(i) };
        let bv = unsafe { *pb.add(i) };
        if av != bv {
            // Promote to i32 to avoid u8 underflow on subtract.
            return av as i32 - bv as i32;
        }
        i += 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_n_is_eq() {
        let a = b"abc";
        let b = b"xyz";
        let r = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 0) };
        assert_eq!(r, 0);
    }

    #[test]
    fn equal() {
        let a = b"hello";
        let b = b"hello";
        let r = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 5) };
        assert_eq!(r, 0);
    }

    #[test]
    fn a_less() {
        let a = b"abc";
        let b = b"abd";
        let r = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 3) };
        assert!(r < 0);
    }

    #[test]
    fn a_greater() {
        let a = b"abd";
        let b = b"abc";
        let r = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 3) };
        assert!(r > 0);
    }

    #[test]
    fn first_byte_diff_only() {
        let a = b"xbc";
        let b = b"abd";
        // First byte differs (x > a); cmp should return positive
        // (the libc memcmp contract only requires sign, not the
        // exact byte-diff value — compiler_builtins' memcmp
        // returns -1/0/1; libc returns the byte diff; our impl
        // returns the byte diff; either passes the libc contract).
        let r = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 3) };
        assert!(r > 0, "expected positive cmp result, got {r}");
    }
}
