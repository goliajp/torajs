//! `memset(s, c, n) -> *mut c_void` — fill `n` bytes at `s` with `c`.
//!
//! LLVM lowers `@llvm.memset.p0.i64` (struct init, `[u8; N]`
//! zeroing, `ptr::write_bytes`, slice `fill`) to a call to the
//! libc-shape `memset` symbol. Without an in-binary definition the
//! linker resolves it against libSystem, pulling libc into the user
//! binary's import table.
//!
//! Same leaf-shim pattern as [`memcpy`](super::memcpy::memcpy):
//! pure byte-level, no syscall / signal / control-flow effect, so
//! it is safe under the force_load 0-libc link (the memory-effect
//! shape matches what LLVM expects for the intrinsic target). The
//! Step 16-a-3 audit found memset is the low-risk half of the pair
//! (memset was never part of the reverted 16-a-2 bzero+abort batch).

use core::ffi::c_void;

use crate::word::{st2, st4, st8};

/// `memset(s, c, n)` — write the low byte of `c`, `n` times.
///
/// Signature matches the libc prototype exactly (`c_void`
/// pointer) — see `memcpy` for the
/// `suspicious_runtime_symbol_definitions` rationale.
///
/// # Safety
///
/// `s` must be writable for ≥ `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(s: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let p = s.cast::<u8>();
    // only the low 8 bits of `c` are used, per libc semantics
    let w = (c as u8 as u64) * 0x0101_0101_0101_0101;
    unsafe {
        if n >= 16 {
            let mut i = 0;
            while n - i >= 16 {
                st8(p.add(i), w);
                st8(p.add(i + 8), w);
                i += 16;
            }
            // the last partial block, as the 16 bytes ending at `n`
            st8(p.add(n - 16), w);
            st8(p.add(n - 8), w);
        } else if n >= 8 {
            st8(p, w);
            st8(p.add(n - 8), w);
        } else if n >= 4 {
            st4(p, w as u32);
            st4(p.add(n - 4), w as u32);
        } else if n >= 2 {
            st2(p, w as u16);
            st2(p.add(n - 2), w as u16);
        } else if n == 1 {
            *p = w as u8;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_n_is_noop() {
        let mut buf = [0xAAu8; 4];
        let r = unsafe { memset(buf.as_mut_ptr().cast(), 0, 0) };
        assert_eq!(r.cast::<u8>(), buf.as_mut_ptr());
        assert_eq!(&buf, &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn fill_zero() {
        let mut buf = [0xFFu8; 8];
        unsafe { memset(buf.as_mut_ptr().cast(), 0, 8) };
        assert_eq!(&buf, &[0u8; 8]);
    }

    #[test]
    fn fill_nonzero() {
        let mut buf = [0u8; 5];
        unsafe { memset(buf.as_mut_ptr().cast(), 0x41, 5) };
        assert_eq!(&buf, b"AAAAA");
    }

    #[test]
    fn only_low_byte_used() {
        // 0x1_42 truncates to 0x42 ('B') — high bits ignored.
        let mut buf = [0u8; 3];
        unsafe { memset(buf.as_mut_ptr().cast(), 0x142, 3) };
        assert_eq!(&buf, b"BBB");
    }

    #[test]
    fn partial() {
        let mut buf = [b'.'; 6];
        unsafe { memset(buf.as_mut_ptr().cast(), b'x' as i32, 3) };
        assert_eq!(&buf, b"xxx...");
    }

    /// Every length across the small / 16-block seams at several
    /// alignments, with guard bytes on both sides.
    #[test]
    fn every_length_and_alignment() {
        for n in 0..=200usize {
            for off in [0usize, 1, 3, 7] {
                let mut buf = [0x11u8; 260];
                unsafe { memset(buf.as_mut_ptr().add(16 + off).cast(), 0xA5, n) };
                assert!(buf[..16 + off].iter().all(|&b| b == 0x11), "n={n} head");
                assert!(
                    buf[16 + off..16 + off + n].iter().all(|&b| b == 0xA5),
                    "n={n}"
                );
                assert!(buf[16 + off + n..].iter().all(|&b| b == 0x11), "n={n} tail");
            }
        }
    }
}
