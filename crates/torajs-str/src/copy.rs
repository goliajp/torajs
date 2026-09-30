//! Payload copies for the concat kernels.
//!
//! The strings a concat joins are mostly short — a word, a key, a
//! separator — and for those a `memcpy` call costs more than the bytes
//! it moves: the call keeps the kernel from being a leaf, and the
//! callee has to find out the length class again. [`copy_short`] moves
//! up to [`SHORT_MAX`] bytes with at most four loads and four stores
//! of fixed width, picked so the first and last of them cover the
//! range from both ends and overlap in the middle. That is the small
//! and medium path of every libc `memcpy`, spelled where the kernel
//! can inline it.

/// Longest copy [`copy_short`] takes: the payload of the largest
/// pooled block.
pub(crate) const SHORT_MAX: usize = crate::layout::STR_POOL_PAYLOAD as usize;

/// Copy `n <= SHORT_MAX` bytes from `src` to `dst` without a call.
///
/// # Safety
///
/// `src` is readable and `dst` writable for `n` bytes, the two ranges
/// do not overlap, and `n <= SHORT_MAX`.
#[inline(always)]
pub(crate) unsafe fn copy_short(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        if n >= 16 {
            let head = (src as *const u128).read_unaligned();
            let tail = (src.add(n - 16) as *const u128).read_unaligned();
            if n > 32 {
                let head2 = (src.add(16) as *const u128).read_unaligned();
                let tail2 = (src.add(n - 32) as *const u128).read_unaligned();
                (dst.add(16) as *mut u128).write_unaligned(head2);
                (dst.add(n - 32) as *mut u128).write_unaligned(tail2);
            }
            (dst as *mut u128).write_unaligned(head);
            (dst.add(n - 16) as *mut u128).write_unaligned(tail);
        } else if n >= 8 {
            let head = (src as *const u64).read_unaligned();
            let tail = (src.add(n - 8) as *const u64).read_unaligned();
            (dst as *mut u64).write_unaligned(head);
            (dst.add(n - 8) as *mut u64).write_unaligned(tail);
        } else if n >= 4 {
            let head = (src as *const u32).read_unaligned();
            let tail = (src.add(n - 4) as *const u32).read_unaligned();
            (dst as *mut u32).write_unaligned(head);
            (dst.add(n - 4) as *mut u32).write_unaligned(tail);
        } else if n >= 2 {
            let head = (src as *const u16).read_unaligned();
            let tail = (src.add(n - 2) as *const u16).read_unaligned();
            (dst as *mut u16).write_unaligned(head);
            (dst.add(n - 2) as *mut u16).write_unaligned(tail);
        } else if n == 1 {
            dst.write(src.read());
        }
    }
}

/// Copy `n` bytes: [`copy_short`] when it applies, `memcpy` past it.
///
/// # Safety
///
/// `src` is readable and `dst` writable for `n` bytes, and the two
/// ranges do not overlap.
#[inline(always)]
pub(crate) unsafe fn copy_bytes(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        if n <= SHORT_MAX {
            copy_short(dst, src, n);
        } else {
            core::ptr::copy_nonoverlapping(src, dst, n);
        }
    }
}

/// Write `src` (Latin-1, one byte a code unit) at `dst` as UTF-16 LE.
///
/// # Safety
///
/// `src` is readable for `units` bytes and `dst` writable for
/// `2 * units`.
#[inline(always)]
pub(crate) unsafe fn widen_latin1(dst: *mut u8, src: *const u8, units: usize) {
    for i in 0..units {
        unsafe { (dst.add(i * 2) as *mut u16).write_unaligned(src.add(i).read() as u16) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn every_short_length_copies_exactly_its_bytes() {
        let src: Vec<u8> = (0..SHORT_MAX as u8 + 8)
            .map(|i| i.wrapping_mul(7) ^ 0x5A)
            .collect();
        for n in 0..=SHORT_MAX {
            for at in [0usize, 1, 3] {
                // guard bytes on both sides must survive
                let mut dst = vec![0xEEu8; SHORT_MAX + 16];
                unsafe { copy_short(dst.as_mut_ptr().add(4), src.as_ptr().add(at), n) };
                assert_eq!(&dst[..4], &[0xEE; 4], "n = {n}");
                assert_eq!(&dst[4..4 + n], &src[at..at + n], "n = {n}");
                assert!(dst[4 + n..].iter().all(|b| *b == 0xEE), "n = {n}");
            }
        }
    }

    #[test]
    fn a_long_copy_goes_through_whole() {
        let src: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        for n in [SHORT_MAX + 1, 100, 300] {
            let mut dst = vec![0xEEu8; 304];
            unsafe { copy_bytes(dst.as_mut_ptr(), src.as_ptr(), n) };
            assert_eq!(&dst[..n], &src[..n]);
            assert!(dst[n..].iter().all(|b| *b == 0xEE));
        }
    }

    #[test]
    fn widening_writes_each_byte_as_a_little_endian_unit() {
        let mut dst = [0xEEu8; 8];
        unsafe { widen_latin1(dst.as_mut_ptr(), [b'a', 0xE9, 0xFF].as_ptr(), 3) };
        assert_eq!(dst, [b'a', 0, 0xE9, 0, 0xFF, 0, 0xEE, 0xEE]);
    }
}
