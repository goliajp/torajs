//! `memcmp(a, b, n) -> i32` — comparison, a word at a time.
//!
//! Returns 0 if equal; <0 if first differing byte in `a` is
//! less than `b`; >0 if greater. Matches libc memcmp's
//! "first differing byte difference" semantics.

use core::ffi::c_void;

use crate::word::ld8;

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
    // SAFETY: caller covenant — a, b readable for n bytes.
    unsafe {
        while n - i >= 8 {
            let (x, y) = (ld8(pa.add(i)), ld8(pb.add(i)));
            if x != y {
                // the first differing byte is the most significant
                // one once the words are read big-endian
                let (x, y) = (u64::from_be(x), u64::from_be(y));
                let shift = 56 - (x ^ y).leading_zeros() / 8 * 8;
                return ((x >> shift) & 0xff) as i32 - ((y >> shift) & 0xff) as i32;
            }
            i += 8;
        }
        while i < n {
            let (av, bv) = (*pa.add(i), *pb.add(i));
            if av != bv {
                return av as i32 - bv as i32;
            }
            i += 1;
        }
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

    /// A single differing byte at every position of every length:
    /// the sign follows that byte, whatever comes after it.
    #[test]
    fn first_difference_decides_at_every_position() {
        for n in 1..=70usize {
            for at in 0..n {
                let a: [u8; 72] = core::array::from_fn(|i| (i * 5 + 9) as u8);
                let mut b = a;
                b[at] = a[at].wrapping_add(1);
                // a later, opposite difference must not matter
                if at + 1 < n {
                    b[at + 1] = a[at + 1].wrapping_sub(1);
                }
                let want = a[at] as i32 - b[at] as i32;
                let got = unsafe { memcmp(a.as_ptr().add(1).cast(), b.as_ptr().add(1).cast(), 0) };
                assert_eq!(got, 0);
                let got = unsafe { memcmp(a.as_ptr().cast(), b.as_ptr().cast(), n) };
                assert_eq!(got, want, "n={n} at={at}");
                let got = unsafe { memcmp(b.as_ptr().cast(), a.as_ptr().cast(), n) };
                assert_eq!(got, -want, "n={n} at={at} swapped");
            }
            let a: [u8; 72] = core::array::from_fn(|i| i as u8);
            assert_eq!(
                unsafe { memcmp(a.as_ptr().cast(), a.as_ptr().cast(), n) },
                0
            );
        }
    }
}
