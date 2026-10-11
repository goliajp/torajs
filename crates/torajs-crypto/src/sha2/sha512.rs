#![forbid(unsafe_code)]

use core::fmt;

use crate::ct::zeroize::zeroize;
use crate::digest::{Digest, sealed};

const BLOCK_LEN: usize = 128;

/// Round constants `K` (FIPS 180-4 §4.2.3): the first 64 bits of the
/// fractional parts of the cube roots of the first 80 primes.
const K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

/// SHA-512 initial hash value (FIPS 180-4 §5.3.5): square roots of the
/// first 8 primes.
const H512: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// SHA-384 initial hash value (FIPS 180-4 §5.3.4): square roots of the
/// 9th through 16th primes.
const H384: [u64; 8] = [
    0xcbbb9d5dc1059ed8,
    0x629a292a367cd507,
    0x9159015a3070dd17,
    0x152fecd8f70e5939,
    0x67332667ffc00b31,
    0x8eb44a8768581511,
    0xdb0c2e0d64f98fa7,
    0x47b5481dbefa4fa4,
];

fn compress(h: &mut [u64; 8], blocks: &[[u8; BLOCK_LEN]]) {
    for block in blocks {
        process_block(h, block);
    }
}

/// One block of the FIPS 180-4 §6.4.2 computation.
fn process_block(h: &mut [u64; 8], block: &[u8; BLOCK_LEN]) {
    let mut w = [0u64; 80];
    for (wi, b) in w.iter_mut().zip(block.as_chunks::<8>().0) {
        *wi = u64::from_be_bytes(*b);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
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

/// The SHA-512 state; SHA-384 is the same computation with another initial
/// value and a truncated output.
#[derive(Clone)]
struct Core {
    h: [u64; 8],
    buf: [u8; BLOCK_LEN],
    buf_len: usize,
    total_len: u128,
}

impl Core {
    const fn new(h: [u64; 8]) -> Core {
        Core {
            h,
            buf: [0; BLOCK_LEN],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u128);
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

    /// FIPS 180-4 §5.1.2 padding: 0x80, zeros up to 112 mod 128, then the
    /// length in bits as a big-endian u128. Writes the first `out.len()`
    /// bytes of the big-endian state and wipes the state.
    fn finish_into(&mut self, out: &mut [u8]) {
        let mut tail = [0u8; 2 * BLOCK_LEN];
        let rest = self.buf_len;
        tail[..rest].copy_from_slice(&self.buf[..rest]);
        tail[rest] = 0x80;
        let n = if rest < 112 { BLOCK_LEN } else { 2 * BLOCK_LEN };
        tail[n - 16..n].copy_from_slice(&self.total_len.wrapping_mul(8).to_be_bytes());
        compress(&mut self.h, tail[..n].as_chunks::<BLOCK_LEN>().0);
        zeroize(&mut tail);
        let mut full = [0u8; 64];
        for (dst, word) in full.as_chunks_mut::<8>().0.iter_mut().zip(self.h) {
            *dst = word.to_be_bytes();
        }
        out.copy_from_slice(&full[..out.len()]);
        zeroize(&mut full);
        self.wipe();
    }

    fn wipe(&mut self) {
        zeroize(&mut self.h);
        zeroize(&mut self.buf);
    }
}

macro_rules! sha512_family {
    ($name:ident, $iv:expr, $len:expr, $oneshot:ident, $doc:literal) => {
        #[doc = concat!("Streaming ", $doc, ".")]
        #[derive(Clone)]
        pub struct $name(Core);

        impl Default for $name {
            fn default() -> Self {
                $name(Core::new($iv))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name))
            }
        }

        impl sealed::Sealed for $name {
            fn wipe(&mut self) {
                self.0.wipe();
            }
        }

        impl Digest for $name {
            const OUTPUT_LEN: usize = $len;
            const BLOCK_LEN: usize = BLOCK_LEN;
            type Output = [u8; $len];

            fn zero_output() -> Self::Output {
                [0; $len]
            }

            fn new() -> Self {
                Self::default()
            }

            fn update(&mut self, data: &[u8]) {
                self.0.update(data);
            }

            fn finish(mut self) -> Self::Output {
                let mut out = [0u8; $len];
                self.0.finish_into(&mut out);
                out
            }
        }

        #[doc = concat!("Hashes `bytes` with ", $doc, " in one call.")]
        pub fn $oneshot(bytes: &[u8]) -> [u8; $len] {
            $name::digest(bytes)
        }
    };
}

sha512_family!(Sha512, H512, 64, sha512, "SHA-512");
sha512_family!(Sha384, H384, 48, sha384, "SHA-384");

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec;
    use std::vec::Vec;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| std::format!("{b:02x}")).collect()
    }

    fn both<D: Digest>(input: &[u8]) -> String {
        let one = D::digest(input);
        let mut st = D::new();
        for piece in input.chunks(53) {
            st.update(piece);
        }
        assert_eq!(st.finish(), one, "streaming disagrees with one-shot");
        hex(one.as_ref())
    }

    const MSG_896: &[u8] = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";

    /// FIPS 180-4 examples (NIST CSRC "Examples with intermediate values").
    #[test]
    fn fips_examples_sha512() {
        assert_eq!(
            both::<Sha512>(b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            both::<Sha512>(MSG_896),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
        assert_eq!(
            both::<Sha512>(&vec![b'a'; 1_000_000]),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
        assert_eq!(
            both::<Sha512>(b""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn fips_examples_sha384() {
        assert_eq!(
            both::<Sha384>(b"abc"),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        );
        assert_eq!(
            both::<Sha384>(MSG_896),
            "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
        );
        assert_eq!(
            both::<Sha384>(&vec![b'a'; 1_000_000]),
            "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
        );
        assert_eq!(
            both::<Sha384>(b""),
            "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
        );
    }

    #[test]
    fn padding_boundaries_and_split_points() {
        let msg: Vec<u8> = (0..400u32).map(|i| (i * 13 + 1) as u8).collect();
        for len in [111, 112, 127, 128, 129, 239, 240, 256] {
            let want = sha512(&msg[..len]);
            for cut in 0..=len {
                let got = Sha512::new()
                    .chain(&msg[..cut])
                    .chain(&msg[cut..len])
                    .finish();
                assert_eq!(got, want, "len {len} cut {cut}");
            }
        }
    }

    #[test]
    fn debug_prints_type_name_only() {
        assert_eq!(std::format!("{:?}", Sha384::new().chain(b"x")), "Sha384");
    }
}
