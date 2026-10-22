//! How many times one byte occurs in a Latin-1 payload — the token
//! count of `s.split(c)` for a one-unit Latin-1 separator, less one.
//!
//! Short payloads (the split lane's dominant shape is a word or a
//! short record) are read eight bytes at a time and every equal byte
//! is found with the exact zero-byte test, so a 13-byte haystack costs
//! two loads instead of thirteen compares. The reads never leave the
//! payload: neither a pooled cell nor a static literal promises
//! readable bytes past its length, so a tail shorter than a word is
//! either taken from the word that ends at the last byte (masking the
//! bytes already counted) or, below eight bytes, assembled from 4-,
//! 2- and 1-byte reads.

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

/// High bit of every byte of `w` equal to the byte broadcast in
/// `needle`, clear elsewhere. Exact: `(x & 0x7f) + 0x7f` never carries
/// out of its byte.
#[inline(always)]
fn eq_bytes(w: u64, needle: u64) -> u64 {
    let x = w ^ needle;
    !(((x & !HI).wrapping_add(!HI)) | x) & HI
}

/// Sum of the byte lanes of `lanes`; the caller keeps the sum below
/// 256.
#[inline(always)]
fn lane_sum(lanes: u64) -> u64 {
    lanes.wrapping_mul(LO) >> 56
}

#[inline(always)]
fn load8(s: &[u8], at: usize) -> u64 {
    debug_assert!(at + 8 <= s.len());
    // SAFETY: every caller reads a word that ends at or before the last
    // byte
    u64::from_le(unsafe { s.as_ptr().add(at).cast::<u64>().read_unaligned() })
}

/// The bytes of a payload shorter than eight bytes, in little-endian
/// order, zero above its length.
#[inline(always)]
fn load_short(s: &[u8]) -> u64 {
    let n = s.len();
    debug_assert!(n < 8);
    let p = s.as_ptr();
    let mut w = 0u64;
    let mut off = 0usize;
    // SAFETY: each read covers bytes `off..off + k` with `off + k <= n`:
    // the bits of `n` taken high to low never sum past `n`
    unsafe {
        if n & 4 != 0 {
            w = u64::from(u32::from_le(p.cast::<u32>().read_unaligned()));
            off = 4;
        }
        if n & 2 != 0 {
            let h = u16::from_le(p.add(off).cast::<u16>().read_unaligned());
            w |= u64::from(h) << (8 * off);
            off += 2;
        }
        if n & 1 != 0 {
            w |= u64::from(*p.add(off)) << (8 * off);
        }
    }
    w
}

/// Count of `b` in `s[from..]`, where `s` holds at least eight bytes
/// and fewer than sixteen remain past `from`.
#[inline(always)]
fn count_rest(s: &[u8], from: usize, needle: u64) -> u64 {
    let n = s.len();
    let mut lanes = 0u64;
    let mut i = from;
    if n - i >= 8 {
        lanes += eq_bytes(load8(s, i), needle) >> 7;
        i += 8;
    }
    let r = n - i;
    if r != 0 {
        // the word ending at the last byte; its low `8 - r` bytes are
        // already counted
        let keep = HI << (8 * (8 - r));
        lanes += (eq_bytes(load8(s, n - 8), needle) & keep) >> 7;
    }
    lane_sum(lanes)
}

/// Count of `b` in `s`, whose length is a multiple of sixteen.
#[cfg(target_arch = "aarch64")]
#[inline]
fn count_bulk(s: &[u8], b: u8) -> u64 {
    use core::arch::aarch64::{vaddlvq_u8, vceqq_u8, vdupq_n_u8, vld1q_u8, vsubq_u8};
    debug_assert!(s.len().is_multiple_of(16));
    let mut total = 0u64;
    // SAFETY: NEON is part of the aarch64 baseline, and every load
    // reads one whole 16-byte chunk of `s`
    unsafe {
        let needle = vdupq_n_u8(b);
        // each lane gains at most one per chunk: flush before it wraps
        for group in s.chunks(16 * 255) {
            let mut acc = vdupq_n_u8(0);
            for c in group.as_chunks::<16>().0 {
                let eq = vceqq_u8(vld1q_u8(c.as_ptr()), needle);
                acc = vsubq_u8(acc, eq);
            }
            total += u64::from(vaddlvq_u8(acc));
        }
    }
    total
}

/// Count of `b` in `s`, whose length is a multiple of sixteen.
#[cfg(not(target_arch = "aarch64"))]
#[inline]
fn count_bulk(s: &[u8], b: u8) -> u64 {
    s.iter().filter(|&&c| c == b).count() as u64
}

/// Number of bytes of `s` equal to `b`.
#[inline(never)]
pub(super) fn count_byte(s: &[u8], b: u8) -> u64 {
    count_byte_inline(s, b)
}

/// [`count_byte`] with the short payloads answered in the caller: the
/// split-count entries take it so the dominant shape costs no second
/// call; payloads of sixteen bytes and more go to [`count_long`].
#[inline(always)]
pub(super) fn count_byte_inline(s: &[u8], b: u8) -> u64 {
    let needle = LO.wrapping_mul(u64::from(b));
    let n = s.len();
    if n < 8 {
        let valid = HI & ((1u64 << (8 * n)) - 1);
        return lane_sum((eq_bytes(load_short(s), needle) & valid) >> 7);
    }
    if n < 16 {
        return count_rest(s, 0, needle);
    }
    count_long(s, b)
}

#[inline(never)]
fn count_long(s: &[u8], b: u8) -> u64 {
    let bulk = s.len() & !15;
    count_bulk(&s[..bulk], b) + count_rest(s, bulk, LO.wrapping_mul(u64::from(b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn naive(s: &[u8], b: u8) -> u64 {
        s.iter().filter(|&&c| c == b).count() as u64
    }

    #[test]
    fn every_length_and_position() {
        for n in 0..=40usize {
            // sep at each single position, then at every position
            for at in 0..n {
                let mut s: Vec<u8> = (0..n).map(|i| b'a' + (i % 26) as u8).collect();
                s[at] = b' ';
                assert_eq!(count_byte(&s, b' '), 1, "n={n} at={at}");
            }
            let all = alloc::vec![b','; n];
            assert_eq!(count_byte(&all, b','), n as u64, "all n={n}");
            assert_eq!(count_byte(&all, b'x'), 0, "none n={n}");
        }
    }

    #[test]
    fn mixed_patterns_match_naive() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for n in (0..=40).chain([64, 255, 256, 4079, 4080, 4096, 9000]) {
            let s: Vec<u8> = (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    [b' ', b'x', 0, 0x80, 0xff, 0x7f][(seed % 6) as usize]
                })
                .collect();
            for b in [b' ', b'x', 0, 0x80, 0xff, 0x7f, b'q'] {
                assert_eq!(count_byte(&s, b), naive(&s, b), "n={n} b={b}");
            }
        }
    }

    #[test]
    fn first_last_and_runs() {
        assert_eq!(count_byte(b"", b' '), 0);
        assert_eq!(count_byte(b" ", b' '), 1);
        assert_eq!(count_byte(b" abc ", b' '), 2);
        assert_eq!(count_byte(b"  ab  ", b' '), 4);
        assert_eq!(count_byte(b"3 4 + 2 * 5 +", b' '), 6);
        assert_eq!(count_byte(b",,,,,,,,,,,,,,,,,", b','), 17);
        // a long run of the byte overflows nothing: lanes flush per group
        let big = alloc::vec![7u8; 16 * 255 * 3 + 5];
        assert_eq!(count_byte(&big, 7), big.len() as u64);
    }

    #[test]
    fn zero_padding_is_not_counted() {
        // load_short leaves zero above the length; a NUL needle must
        // not see it
        for n in 0..8 {
            let s = alloc::vec![1u8; n];
            assert_eq!(count_byte(&s, 0), 0, "n={n}");
        }
    }
}
