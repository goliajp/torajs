//! Portable GCM: bitsliced AES in counter mode and a table-free GHASH.
//!
//! Field elements are kept as two `u64` limbs in the natural polynomial
//! order (bit `i` of the 128-bit value is the coefficient of `x^i`). A
//! GHASH block stores the coefficient of `x^0` in the top bit of its first
//! byte, so converting a block is a bit reversal of each big-endian half.
//! The 64x64 carry-less product uses integer multiplications on operands
//! with every fourth bit kept, so carries land in the gaps and are masked
//! off (the `bmul64` construction of BearSSL); no lookup depends on data.
#![forbid(unsafe_code)]

use crate::aes::{BLOCK_LEN, soft};
use crate::ct::zeroize::zeroize;

/// A GHASH block in natural polynomial order.
pub(super) fn load(b: &[u8; BLOCK_LEN]) -> [u64; 2] {
    let half = |i: usize| u64::from_be_bytes(b[i..i + 8].try_into().unwrap()).reverse_bits();
    [half(0), half(8)]
}

pub(super) fn store(y: [u64; 2]) -> [u8; BLOCK_LEN] {
    let mut b = [0u8; BLOCK_LEN];
    b[..8].copy_from_slice(&y[0].reverse_bits().to_be_bytes());
    b[8..].copy_from_slice(&y[1].reverse_bits().to_be_bytes());
    b
}

/// Low 64 bits of the carry-less product.
fn bmul64(x: u64, y: u64) -> u64 {
    const M: [u64; 4] = [
        0x1111_1111_1111_1111,
        0x2222_2222_2222_2222,
        0x4444_4444_4444_4444,
        0x8888_8888_8888_8888,
    ];
    let xs = M.map(|m| x & m);
    let ys = M.map(|m| y & m);
    let mut z = 0;
    for (k, m) in M.iter().enumerate() {
        let mut acc = 0u64;
        for (i, xi) in xs.iter().enumerate() {
            acc ^= xi.wrapping_mul(ys[(k + 4 - i) % 4]);
        }
        z |= acc & m;
    }
    z
}

/// The full 128-bit carry-less product `(low, high)`. The high half comes
/// from the product of the bit-reversed operands.
fn clmul(x: u64, y: u64) -> (u64, u64) {
    let lo = bmul64(x, y);
    let hi = bmul64(x.reverse_bits(), y.reverse_bits()).reverse_bits() >> 1;
    (lo, hi)
}

/// Reduces a 256-bit product modulo `x^128 + x^7 + x^2 + x + 1`.
pub(super) fn reduce(z: [u64; 4]) -> [u64; 2] {
    let [mut z0, mut z1, mut z2, z3] = z;
    // x^192 = x^64 (x^7 + x^2 + x + 1), then x^128 = x^7 + x^2 + x + 1
    z1 ^= z3 ^ (z3 << 1) ^ (z3 << 2) ^ (z3 << 7);
    z2 ^= (z3 >> 63) ^ (z3 >> 62) ^ (z3 >> 57);
    z0 ^= z2 ^ (z2 << 1) ^ (z2 << 2) ^ (z2 << 7);
    z1 ^= (z2 >> 63) ^ (z2 >> 62) ^ (z2 >> 57);
    [z0, z1]
}

/// Multiplication in GF(2^128), Karatsuba over the 64-bit halves.
pub(super) fn mul(a: [u64; 2], b: [u64; 2]) -> [u64; 2] {
    let (l0, l1) = clmul(a[0], b[0]);
    let (h0, h1) = clmul(a[1], b[1]);
    let (m0, m1) = clmul(a[0] ^ a[1], b[0] ^ b[1]);
    let (m0, m1) = (m0 ^ l0 ^ h0, m1 ^ l1 ^ h1);
    reduce([l0, l1 ^ m0, h0 ^ m1, h1])
}

/// Keyed portable GCM state: bitsliced round keys and the hash key `H`.
pub(super) struct Gcm {
    aes: soft::Keys,
    h: [u64; 2],
}

impl Gcm {
    pub(super) fn new(words: &[u32], rounds: usize) -> Self {
        let aes = soft::Keys::new(&words[..4 * (rounds + 1)], rounds);
        let mut gcm = Gcm { aes, h: [0; 2] };
        let mut h = gcm.encrypt_block([0; BLOCK_LEN]);
        gcm.h = load(&h);
        zeroize(&mut h);
        gcm
    }

    pub(super) fn wipe(&mut self) {
        self.aes.wipe();
        zeroize(&mut self.h);
    }

    pub(super) fn encrypt_block(&self, b: [u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
        let mut blocks = [b, [0; BLOCK_LEN], [0; BLOCK_LEN], [0; BLOCK_LEN]];
        self.aes.encrypt4(&mut blocks);
        blocks[0]
    }

    /// Absorbs `data` into `y`, zero-padding a final partial block.
    pub(super) fn ghash(&self, y: &mut [u64; 2], data: &[u8]) {
        for chunk in data.chunks(BLOCK_LEN) {
            let mut block = [0u8; BLOCK_LEN];
            block[..chunk.len()].copy_from_slice(chunk);
            let x = load(&block);
            *y = mul([y[0] ^ x[0], y[1] ^ x[1]], self.h);
        }
    }

    /// XORs `data` with the keystream for counter blocks
    /// `prefix || ctr`, `prefix || ctr + 1`, ...
    fn ctr_chunk(&self, prefix: &[u8; 12], ctr: u32, chunk: &mut [u8]) {
        let mut ks = [[0u8; BLOCK_LEN]; 4];
        for (i, block) in ks.iter_mut().enumerate() {
            block[..12].copy_from_slice(prefix);
            block[12..].copy_from_slice(&ctr.wrapping_add(i as u32).to_be_bytes());
        }
        self.aes.encrypt4(&mut ks);
        for (d, k) in chunk.iter_mut().zip(ks.as_flattened()) {
            *d ^= k;
        }
    }

    /// Encrypts `data` in place and absorbs the ciphertext into `y`.
    pub(super) fn seal(&self, prefix: &[u8; 12], ctr: u32, y: &mut [u64; 2], data: &mut [u8]) {
        let mut ctr = ctr;
        for chunk in data.chunks_mut(4 * BLOCK_LEN) {
            self.ctr_chunk(prefix, ctr, chunk);
            self.ghash(y, chunk);
            ctr = ctr.wrapping_add(4);
        }
    }

    /// Absorbs the ciphertext in `data` into `y` and decrypts it in place.
    pub(super) fn open(&self, prefix: &[u8; 12], ctr: u32, y: &mut [u64; 2], data: &mut [u8]) {
        let mut ctr = ctr;
        for chunk in data.chunks_mut(4 * BLOCK_LEN) {
            self.ghash(y, chunk);
            self.ctr_chunk(prefix, ctr, chunk);
            ctr = ctr.wrapping_add(4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit-serial reference multiplication, straight from SP 800-38D
    /// Algorithm 1, on blocks.
    fn mul_reference(x: [u8; 16], y: [u8; 16]) -> [u8; 16] {
        let mut z = 0u128;
        let mut v = u128::from_be_bytes(y);
        let x = u128::from_be_bytes(x);
        for i in 0..128 {
            if (x >> (127 - i)) & 1 == 1 {
                z ^= v;
            }
            let lsb = v & 1;
            v >>= 1;
            if lsb == 1 {
                v ^= 0xe1 << 120;
            }
        }
        z.to_be_bytes()
    }

    #[test]
    fn mul_matches_the_bit_serial_definition() {
        let mut s = 0x0123_4567_89ab_cdefu64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..2000 {
            let a: [u8; 16] = (u128::from(next()) << 64 | u128::from(next())).to_be_bytes();
            let b: [u8; 16] = (u128::from(next()) << 64 | u128::from(next())).to_be_bytes();
            assert_eq!(store(mul(load(&a), load(&b))), mul_reference(a, b));
        }
        let one = [0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(load(&one), [1, 0]);
        let all = [0xff; 16];
        assert_eq!(store(mul(load(&all), load(&one))), all);
    }
}
