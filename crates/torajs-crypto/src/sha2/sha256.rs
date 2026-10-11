#![forbid(unsafe_code)]

use core::fmt;

use crate::ct::zeroize::zeroize;
use crate::digest::{Digest, sealed};

/// SHA-256 digest length in bytes.
const DIGEST_LEN: usize = 32;

pub(super) const BLOCK_LEN: usize = 64;

/// Round constants `K` (FIPS 180-4 §4.2.2): the first 32 bits of the
/// fractional parts of the cube roots of the first 64 primes.
pub(super) const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash value `H(0)` (FIPS 180-4 §5.3.3): the first 32 bits of the
/// fractional parts of the square roots of the first 8 primes.
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Hashes `bytes` with SHA-256 in one call.
///
/// ```
/// let d = torajs_crypto::sha2::sha256(b"abc");
/// assert_eq!(d[..4], [0xba, 0x78, 0x16, 0xbf]);
/// ```
pub fn sha256(bytes: &[u8]) -> [u8; DIGEST_LEN] {
    let mut h = H0;
    let (full, rest) = bytes.as_chunks::<BLOCK_LEN>();
    compress(&mut h, full);
    finish_tail(h, rest, bytes.len() as u64)
}

/// FIPS 180-4 §5.1.1 padding of the final partial block `rest`: append
/// 0x80, zeros up to 56 mod 64, then the message length in bits as a
/// big-endian u64.
fn finish_tail(mut h: [u32; 8], rest: &[u8], total_len: u64) -> [u8; DIGEST_LEN] {
    let mut tail = [0u8; 2 * BLOCK_LEN];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let n = if rest.len() < 56 {
        BLOCK_LEN
    } else {
        2 * BLOCK_LEN
    };
    tail[n - 8..n].copy_from_slice(&total_len.wrapping_mul(8).to_be_bytes());
    compress(&mut h, tail[..n].as_chunks::<BLOCK_LEN>().0);
    zeroize(&mut tail);

    let mut digest = [0u8; DIGEST_LEN];
    for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *out = word.to_be_bytes();
    }
    digest
}

#[cfg(all(
    target_arch = "aarch64",
    target_feature = "sha2",
    not(torajs_crypto_portable)
))]
fn compress(h: &mut [u32; 8], blocks: &[[u8; BLOCK_LEN]]) {
    super::aarch64::compress(h, blocks);
}

#[cfg(not(all(
    target_arch = "aarch64",
    target_feature = "sha2",
    not(torajs_crypto_portable)
)))]
fn compress(h: &mut [u32; 8], blocks: &[[u8; BLOCK_LEN]]) {
    for block in blocks {
        process_block(h, block);
    }
}

/// One block of the FIPS 180-4 §6.2.2 computation.
#[cfg_attr(
    all(
        target_arch = "aarch64",
        target_feature = "sha2",
        not(torajs_crypto_portable),
        not(test)
    ),
    allow(dead_code)
)]
fn process_block(h: &mut [u32; 8], block: &[u8; BLOCK_LEN]) {
    let mut w = [0u32; 64];
    for (wi, b) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *wi = u32::from_be_bytes(*b);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    for (s, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *s = s.wrapping_add(v);
    }
}

/// Streaming SHA-256.
///
/// ```
/// use torajs_crypto::Digest;
/// use torajs_crypto::sha2::{Sha256, sha256};
///
/// let d = Sha256::new().chain(b"ab").chain(b"c").finish();
/// assert_eq!(d, sha256(b"abc"));
/// ```
#[derive(Clone)]
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; BLOCK_LEN],
    buf_len: usize,
    total_len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: H0,
            buf: [0; BLOCK_LEN],
            buf_len: 0,
            total_len: 0,
        }
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sha256")
    }
}

impl sealed::Sealed for Sha256 {
    fn wipe(&mut self) {
        zeroize(&mut self.h);
        zeroize(&mut self.buf);
    }
}

impl Digest for Sha256 {
    const OUTPUT_LEN: usize = DIGEST_LEN;
    const BLOCK_LEN: usize = BLOCK_LEN;
    type Output = [u8; DIGEST_LEN];

    fn zero_output() -> Self::Output {
        [0; DIGEST_LEN]
    }

    fn new() -> Self {
        Self::default()
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (BLOCK_LEN - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < BLOCK_LEN {
                return;
            }
            compress(&mut self.h, core::slice::from_ref(&self.buf));
            self.buf_len = 0;
        }
        let (full, rest) = data.as_chunks::<BLOCK_LEN>();
        compress(&mut self.h, full);
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }

    fn finish(mut self) -> Self::Output {
        let out = finish_tail(self.h, &self.buf[..self.buf_len], self.total_len);
        sealed::Sealed::wipe(&mut self);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| std::format!("{b:02x}")).collect()
    }

    fn both(input: &[u8]) -> String {
        let one = sha256(input);
        // feed in uneven pieces so the buffer path runs too
        let mut st = Sha256::new();
        for piece in input.chunks(37) {
            st.update(piece);
        }
        assert_eq!(st.finish(), one, "streaming disagrees with one-shot");
        hex(&one)
    }

    /// FIPS 180-4 examples (NIST CSRC "Examples with intermediate values").
    #[test]
    fn fips_examples() {
        assert_eq!(
            both(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            both(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            both(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(
            both(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// Values the codesigner relied on before this crate existed; pinning
    /// them keeps its page hashes byte-identical.
    #[test]
    fn previously_pinned_values() {
        let block: Vec<u8> = (0..64).map(|i| i as u8).collect();
        assert_eq!(
            both(&block),
            "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108"
        );
        assert_eq!(
            both(&vec![b'a'; 1000]),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
        assert_eq!(
            both(b"a"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
        assert_eq!(
            both(&vec![0u8; 4096]),
            "ad7facb2586fc6e966c004d7d1d16b024f5805ff7cb47c7a85dabd8b48892ca7"
        );
        let a = [b'a'; 120];
        for (len, want) in [
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                119,
                "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
            ),
        ] {
            assert_eq!(both(&a[..len]), want, "len {len}");
        }
    }

    /// The block function in use and the FIPS rounds agree on blocks that
    /// exercise every schedule word.
    #[test]
    fn compress_matches_the_scalar_rounds() {
        let mut seed = 0x9e37_79b9u32;
        let blocks: Vec<[u8; BLOCK_LEN]> = (0..37)
            .map(|_| {
                core::array::from_fn(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
            })
            .collect();
        let mut fast = H0;
        compress(&mut fast, &blocks);
        let mut slow = H0;
        for b in &blocks {
            process_block(&mut slow, b);
        }
        assert_eq!(fast, slow);
    }

    #[test]
    fn every_split_point_agrees() {
        let msg: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        let want = sha256(&msg);
        for cut in 0..msg.len() {
            let got = Sha256::new().chain(&msg[..cut]).chain(&msg[cut..]).finish();
            assert_eq!(got, want, "cut {cut}");
        }
    }
}
