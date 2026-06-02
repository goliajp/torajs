//! `memset_pattern16(b, pattern16, len) -> void` — fill `len` bytes
//! at `b` with successive copies of the 16-byte pattern at
//! `pattern16` (truncated if `len % 16 != 0`).
//!
//! Darwin-only POSIX extension (`<string.h>`,
//! `memset_pattern{4,8,16}`). LLVM's loop-idiom recognition pass
//! lowers `[u8; N]`-style initializers and tight repeat-store loops
//! to a call to `memset_pattern16` when the source pattern is a
//! 16-byte constant (e.g. hash bucket arrays initialized to a
//! sentinel like `0xFFFF_FFFF_FFFF_FFFF`). Without an in-binary
//! definition the linker resolves it against libSystem, pulling
//! libc into the user binary's import table — this shim drops it.
//!
//! Same leaf-shim contract as [`memset`](super::memset::memset):
//! pure byte-level writes, no syscall / signal / control-flow
//! effect, force-load safe. The two-NEON-store inner loop is the
//! same shape darwin's libsystem impl uses; LLVM's idiom-matching
//! pass auto-vectorizes the unrolled u128 store into `stp q,q`.

/// `memset_pattern16(b, pattern16, len)` — fill `len` bytes at `b`
/// with copies of the 16 bytes at `pattern16`. If `len` is not a
/// multiple of 16, the trailing partial chunk is filled with the
/// first `len % 16` bytes of the pattern (matches Darwin libc).
///
/// # Safety
///
/// `b` must be writable for ≥ `len` bytes. `pattern16` must point
/// to ≥ 16 readable bytes. The two regions must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset_pattern16(b: *mut u8, pattern16: *const u8, len: usize) {
    // Read the 16-byte pattern once as a u128 to let LLVM unroll
    // the inner loop into a single 16-byte NEON `stp q,q` (or two
    // 8-byte stores on cores without 128-bit pairs). The pattern
    // region is small and read-only across the call, so the load
    // hoists trivially.
    let pat = unsafe { (pattern16 as *const u128).read_unaligned() };
    let full_chunks = len / 16;
    let mut i = 0;
    while i < full_chunks {
        unsafe {
            (b.add(i * 16) as *mut u128).write_unaligned(pat);
        }
        i += 1;
    }
    // Tail: `len % 16` leading bytes of the pattern.
    let tail = len & 15;
    if tail > 0 {
        let dst_tail = unsafe { b.add(full_chunks * 16) };
        let mut j = 0;
        while j < tail {
            unsafe {
                *dst_tail.add(j) = *pattern16.add(j);
            }
            j += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_len_is_noop() {
        let mut buf = [0xAAu8; 8];
        let pat = [0x55u8; 16];
        unsafe { memset_pattern16(buf.as_mut_ptr(), pat.as_ptr(), 0) };
        assert_eq!(&buf, &[0xAA; 8]);
    }

    #[test]
    fn single_chunk_exact_16() {
        let mut buf = [0u8; 16];
        let pat: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        unsafe { memset_pattern16(buf.as_mut_ptr(), pat.as_ptr(), 16) };
        assert_eq!(buf, pat);
    }

    #[test]
    fn multiple_chunks() {
        let mut buf = [0u8; 48];
        let pat: [u8; 16] = [0xAB; 16];
        unsafe { memset_pattern16(buf.as_mut_ptr(), pat.as_ptr(), 48) };
        assert_eq!(&buf, &[0xAB; 48]);
    }

    #[test]
    fn partial_tail() {
        let mut buf = [0u8; 20];
        let pat: [u8; 16] = [
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
            0xAE, 0xAF,
        ];
        unsafe { memset_pattern16(buf.as_mut_ptr(), pat.as_ptr(), 20) };
        // First 16 bytes: full pattern. Last 4: pattern[0..4].
        let mut expected = [0u8; 20];
        expected[..16].copy_from_slice(&pat);
        expected[16..].copy_from_slice(&pat[..4]);
        assert_eq!(buf, expected);
    }

    #[test]
    fn tail_smaller_than_chunk() {
        let mut buf = [0xFFu8; 7];
        let pat: [u8; 16] = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
            0xFF, 0x00,
        ];
        unsafe { memset_pattern16(buf.as_mut_ptr(), pat.as_ptr(), 7) };
        assert_eq!(&buf, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
    }
}
