//! The Poly1305 one-time authenticator (RFC 8439 §2.5).
//!
//! The accumulator lives in three limbs of 44, 44 and 42 bits, so each
//! limb product fits a `u128` with room for the sums. Carries are
//! propagated with shifts and masks; no limb value decides a branch.
#![forbid(unsafe_code)]

use crate::ct::zeroize::zeroize;

pub(crate) const BLOCK_LEN: usize = 16;
pub(crate) const KEY_LEN: usize = 32;

const M44: u64 = (1 << 44) - 1;
const M42: u64 = (1 << 42) - 1;

/// A Poly1305 computation keyed with a one-time key. The key-derived
/// state is wiped on drop.
pub(crate) struct Poly1305 {
    r: [u64; 3],
    h: [u64; 3],
    pad: [u64; 2],
    buf: [u8; BLOCK_LEN],
    buf_len: usize,
}

impl Drop for Poly1305 {
    fn drop(&mut self) {
        zeroize(&mut self.r);
        zeroize(&mut self.h);
        zeroize(&mut self.pad);
        zeroize(&mut self.buf);
    }
}

fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap())
}

impl Poly1305 {
    pub(crate) fn new(key: &[u8; KEY_LEN]) -> Self {
        let t0 = le64(&key[0..8]);
        let t1 = le64(&key[8..16]);
        // clamping (RFC 8439 §2.5) folded into the limb split
        let r = [
            t0 & 0x0ffc_0fff_ffff,
            ((t0 >> 44) | (t1 << 20)) & 0x0fff_ffc0_ffff,
            (t1 >> 24) & 0x0f_ffff_fc0f,
        ];
        Poly1305 {
            r,
            h: [0; 3],
            pad: [le64(&key[16..24]), le64(&key[24..32])],
            buf: [0; BLOCK_LEN],
            buf_len: 0,
        }
    }

    /// Absorbs whole 16-byte blocks; `hibit` is `1 << 40` for full blocks
    /// and 0 for the padded final block, whose 0x01 byte is already in place.
    fn blocks(&mut self, data: &[[u8; BLOCK_LEN]], hibit: u64) {
        let [r0, r1, r2] = self.r;
        let s1 = r1 * (5 << 2);
        let s2 = r2 * (5 << 2);
        let [mut h0, mut h1, mut h2] = self.h;
        for block in data {
            let t0 = le64(&block[0..8]);
            let t1 = le64(&block[8..16]);
            h0 += t0 & M44;
            h1 += ((t0 >> 44) | (t1 << 20)) & M44;
            h2 += ((t1 >> 24) & M42) | hibit;

            let mul = |a: u64, b: u64| u128::from(a) * u128::from(b);
            let d0 = mul(h0, r0) + mul(h1, s2) + mul(h2, s1);
            let mut d1 = mul(h0, r1) + mul(h1, r0) + mul(h2, s2);
            let mut d2 = mul(h0, r2) + mul(h1, r1) + mul(h2, r0);

            let c = (d0 >> 44) as u64;
            h0 = d0 as u64 & M44;
            d1 += u128::from(c);
            let c = (d1 >> 44) as u64;
            h1 = d1 as u64 & M44;
            d2 += u128::from(c);
            let c = (d2 >> 42) as u64;
            h2 = d2 as u64 & M42;
            h0 += c * 5;
            let c = h0 >> 44;
            h0 &= M44;
            h1 += c;
        }
        self.h = [h0, h1, h2];
    }

    /// Absorbs `data` as a message stream.
    pub(crate) fn update(&mut self, mut data: &[u8]) {
        if self.buf_len > 0 {
            let take = (BLOCK_LEN - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < BLOCK_LEN {
                return;
            }
            let buf = self.buf;
            self.blocks(&[buf], 1 << 40);
            self.buf_len = 0;
        }
        let (blocks, rest) = data.as_chunks::<BLOCK_LEN>();
        self.blocks(blocks, 1 << 40);
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }

    /// Absorbs `data` followed by zero bytes up to the next multiple of
    /// 16, the `pad16` of the RFC 8439 AEAD construction. Requires that
    /// no partial block is buffered.
    pub(crate) fn update_padded(&mut self, data: &[u8]) {
        debug_assert_eq!(self.buf_len, 0);
        let (blocks, rest) = data.as_chunks::<BLOCK_LEN>();
        self.blocks(blocks, 1 << 40);
        if !rest.is_empty() {
            let mut block = [0u8; BLOCK_LEN];
            block[..rest.len()].copy_from_slice(rest);
            self.blocks(&[block], 1 << 40);
        }
    }

    pub(crate) fn finish(mut self) -> [u8; BLOCK_LEN] {
        if self.buf_len > 0 {
            let mut block = [0u8; BLOCK_LEN];
            block[..self.buf_len].copy_from_slice(&self.buf[..self.buf_len]);
            block[self.buf_len] = 1;
            self.blocks(&[block], 0);
        }
        let [mut h0, mut h1, mut h2] = self.h;

        // full carry: h < 2^130 + small
        let mut c = h1 >> 44;
        h1 &= M44;
        for _ in 0..2 {
            h2 += c;
            c = h2 >> 42;
            h2 &= M42;
            h0 += c * 5;
            c = h0 >> 44;
            h0 &= M44;
            h1 += c;
            c = h1 >> 44;
            h1 &= M44;
        }
        h2 += c;

        // g = h + 5 - 2^130; keep g when it did not borrow, i.e. h >= p
        let mut g0 = h0 + 5;
        let c = g0 >> 44;
        g0 &= M44;
        let mut g1 = h1 + c;
        let c = g1 >> 44;
        g1 &= M44;
        let g2 = (h2 + c).wrapping_sub(1 << 42);
        let keep_g = (g2 >> 63).wrapping_sub(1);
        h0 = (h0 & !keep_g) | (g0 & keep_g);
        h1 = (h1 & !keep_g) | (g1 & keep_g);
        h2 = (h2 & !keep_g) | (g2 & keep_g);

        // tag = (h + s) mod 2^128
        let [s0, s1] = self.pad;
        h0 += s0 & M44;
        let c = h0 >> 44;
        h0 &= M44;
        h1 += (((s0 >> 44) | (s1 << 20)) & M44) + c;
        let c = h1 >> 44;
        h1 &= M44;
        h2 += ((s1 >> 24) & M42) + c;
        h2 &= M42;

        let lo = h0 | (h1 << 44);
        let hi = (h1 >> 20) | (h2 << 24);
        let mut tag = [0u8; BLOCK_LEN];
        tag[..8].copy_from_slice(&lo.to_le_bytes());
        tag[8..].copy_from_slice(&hi.to_le_bytes());
        tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{hex, unhex};

    fn mac(key: &str, msg: &[u8]) -> std::string::String {
        let key: [u8; 32] = unhex(key).try_into().unwrap();
        let mut p = Poly1305::new(&key);
        p.update(msg);
        hex(&p.finish())
    }

    /// RFC 8439 §2.5.2.
    #[test]
    fn rfc8439_example() {
        let key = "85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b";
        assert_eq!(
            mac(key, b"Cryptographic Forum Research Group"),
            "a8061dc1305136c6c22b8baf0c0127a9"
        );
    }

    /// RFC 8439 Appendix A.3, test vector #1: an all-zero key gives an
    /// all-zero tag.
    #[test]
    fn appendix_a3_zero_key() {
        assert_eq!(
            mac(&"00".repeat(32), &[0; 64]),
            "00000000000000000000000000000000"
        );
    }

    #[test]
    fn split_updates_match_one_shot() {
        let key: [u8; 32] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        let msg: std::vec::Vec<u8> = (0..200u32).map(|i| (i * 13) as u8).collect();
        let mut whole = Poly1305::new(&key);
        whole.update(&msg);
        let want = whole.finish();
        for split in [0, 1, 15, 16, 17, 33, 199] {
            let mut p = Poly1305::new(&key);
            p.update(&msg[..split]);
            p.update(&msg[split..]);
            assert_eq!(p.finish(), want, "split {split}");
        }
    }
}
