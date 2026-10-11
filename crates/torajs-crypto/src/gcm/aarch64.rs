//! GCM on the ARMv8 AES and PMULL instructions.
//!
//! Field elements use the natural polynomial order of [`soft`](super::soft):
//! `rbit` on each byte of a GHASH block yields it directly, so the 64x64
//! `pmull` products need no shifting, and the reduction folds the top two
//! 64-bit limbs with two more `pmull`s by `x^7 + x^2 + x + 1`. Up to eight
//! blocks are hashed per reduction with the precomputed powers
//! `H^1..H^8`, and counter-mode encryption of the same blocks is
//! interleaved with the hashing.
//!
//! A [`Gcm`] is only constructed when the caller's
//! [`CpuFeatures`] has both [`Feature::Aarch64Aes`] and
//! [`Feature::Aarch64Pmull`]; that is the invariant every `// SAFETY:`
//! comment below relies on.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use crate::aes::BLOCK_LEN;
use crate::aes::aarch64::{Keys, Loaded};
use crate::cpu::{CpuFeatures, Feature};
use crate::ct::zeroize::zeroize;

/// Blocks per GHASH reduction on the main path.
const WIDE: usize = 8;

/// Keyed GCM state: round keys and `H^1..H^8` in natural order.
pub(super) struct Gcm {
    aes: Keys,
    h: [[u64; 2]; WIDE],
}

/// `H^1..H^8` in registers, each with the XOR of its halves in lane 0 for
/// the Karatsuba middle product.
struct Powers {
    h: [uint64x2_t; WIDE],
    fold: [uint64x2_t; WIDE],
}

#[inline]
#[target_feature(enable = "aes")]
fn pmull_lo(a: uint64x2_t, b: uint64x2_t) -> uint64x2_t {
    vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64::<0>(a), vgetq_lane_u64::<0>(b)))
}

#[inline]
#[target_feature(enable = "aes")]
fn pmull_hi(a: uint64x2_t, b: uint64x2_t) -> uint64x2_t {
    vreinterpretq_u64_p128(vmull_high_p64(
        vreinterpretq_p64_u64(a),
        vreinterpretq_p64_u64(b),
    ))
}

#[inline]
#[target_feature(enable = "aes")]
fn fold(a: uint64x2_t) -> uint64x2_t {
    veorq_u64(a, vextq_u64::<1>(a, a))
}

/// A GHASH block in natural order.
#[inline]
#[target_feature(enable = "aes")]
fn natural(b: uint8x16_t) -> uint64x2_t {
    vreinterpretq_u64_u8(vrbitq_u8(b))
}

/// Unreduced sum of products: low, Karatsuba middle and high 128 bits.
#[derive(Clone, Copy)]
struct Acc {
    lo: uint64x2_t,
    mid: uint64x2_t,
    hi: uint64x2_t,
}

impl Acc {
    #[inline]
    #[target_feature(enable = "aes")]
    fn new(x: uint64x2_t, h: uint64x2_t, hf: uint64x2_t) -> Acc {
        Acc {
            lo: pmull_lo(x, h),
            mid: pmull_lo(fold(x), hf),
            hi: pmull_hi(x, h),
        }
    }

    #[inline]
    #[target_feature(enable = "aes")]
    fn add(&mut self, x: uint64x2_t, h: uint64x2_t, hf: uint64x2_t) {
        self.lo = veorq_u64(self.lo, pmull_lo(x, h));
        self.mid = veorq_u64(self.mid, pmull_lo(fold(x), hf));
        self.hi = veorq_u64(self.hi, pmull_hi(x, h));
    }

    /// Combines the three products into 256 bits and reduces modulo
    /// `x^128 + x^7 + x^2 + x + 1`.
    #[inline]
    #[target_feature(enable = "aes")]
    fn reduce(self) -> uint64x2_t {
        let zero = vdupq_n_u64(0);
        let mid = veorq_u64(self.mid, veorq_u64(self.lo, self.hi));
        let lo = veorq_u64(self.lo, vextq_u64::<1>(zero, mid));
        let hi = veorq_u64(self.hi, vextq_u64::<1>(mid, zero));
        let g = 0x87;
        // x^192 = x^64 (x^7 + x^2 + x + 1)
        let t = vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64::<1>(hi), g));
        let lo = veorq_u64(lo, vextq_u64::<1>(zero, t));
        let hi = veorq_u64(hi, vextq_u64::<1>(t, zero));
        // x^128 = x^7 + x^2 + x + 1
        let u = vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64::<0>(hi), g));
        veorq_u64(lo, u)
    }
}

impl Powers {
    #[inline]
    #[target_feature(enable = "aes")]
    fn load(h: &[[u64; 2]; WIDE]) -> Powers {
        let mut p = Powers {
            h: [vdupq_n_u64(0); WIDE],
            fold: [vdupq_n_u64(0); WIDE],
        };
        for (i, hi) in h.iter().enumerate() {
            // SAFETY: the pointer addresses two `u64`s.
            p.h[i] = unsafe { vld1q_u64(hi.as_ptr()) };
            p.fold[i] = fold(p.h[i]);
        }
        p
    }

    /// `(y ^ x[0]) * H^N ^ x[1] * H^(N-1) ^ ... ^ x[N-1] * H`, one
    /// reduction.
    #[inline]
    #[target_feature(enable = "aes")]
    fn hash<const N: usize>(&self, y: uint64x2_t, x: &[uint64x2_t; N]) -> uint64x2_t {
        let mut acc = Acc::new(veorq_u64(y, x[0]), self.h[N - 1], self.fold[N - 1]);
        for (i, &xi) in x.iter().enumerate().skip(1) {
            acc.add(xi, self.h[N - 1 - i], self.fold[N - 1 - i]);
        }
        acc.reduce()
    }
}

/// Loads 16 bytes from `p`.
///
/// # Safety
///
/// `p` must be valid for reading 16 bytes.
#[inline]
#[target_feature(enable = "aes")]
unsafe fn ld(p: *const u8) -> uint8x16_t {
    // SAFETY: forwarded from the caller.
    unsafe { vld1q_u8(p) }
}

#[inline]
#[target_feature(enable = "aes")]
fn counter(base: uint32x4_t, ctr: u32) -> uint8x16_t {
    vreinterpretq_u8_u32(vsetq_lane_u32::<3>(ctr.to_be(), base))
}

#[inline]
#[target_feature(enable = "aes")]
fn counter_base(prefix: &[u8; 12]) -> uint32x4_t {
    let mut b = [0u8; BLOCK_LEN];
    b[..12].copy_from_slice(prefix);
    // SAFETY: `b` is 16 bytes.
    vreinterpretq_u32_u8(unsafe { ld(b.as_ptr()) })
}

impl Gcm {
    /// Builds the hardware state, or `None` when `cpu` does not vouch for
    /// both AES and PMULL.
    pub(super) fn new(words: &[u32], rounds: usize, cpu: CpuFeatures) -> Option<Self> {
        if !(cpu.has(Feature::Aarch64Aes) && cpu.has(Feature::Aarch64Pmull)) {
            return None;
        }
        let mut gcm = Gcm {
            aes: Keys::new(&words[..4 * (rounds + 1)], rounds),
            h: [[0; 2]; WIDE],
        };
        // SAFETY: `cpu` vouches for FEAT_AES and FEAT_PMULL (the contract
        // of `CpuFeatures::with`), which is all `init_powers` executes.
        unsafe { gcm.init_powers() };
        Some(gcm)
    }

    pub(super) fn wipe(&mut self) {
        self.aes.wipe();
        zeroize(self.h.as_flattened_mut());
    }

    #[target_feature(enable = "aes")]
    fn init_powers(&mut self) {
        let k = self.aes.load();
        let h = natural(k.encrypt(vdupq_n_u8(0)));
        let hf = fold(h);
        let mut p = h;
        for slot in &mut self.h {
            // SAFETY: the pointer addresses two `u64`s.
            unsafe { vst1q_u64(slot.as_mut_ptr(), p) };
            p = Acc::new(p, h, hf).reduce();
        }
    }

    pub(super) fn encrypt_block(&self, b: [u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
        // SAFETY: a `Gcm` exists only when the CPU has FEAT_AES and
        // FEAT_PMULL (see `new`).
        unsafe { self.encrypt_block_hw(b) }
    }

    pub(super) fn ghash(&self, y: &mut [u64; 2], data: &[u8]) {
        // SAFETY: as in `encrypt_block`.
        unsafe { self.ghash_hw(y, data) }
    }

    pub(super) fn seal(&self, prefix: &[u8; 12], ctr: u32, y: &mut [u64; 2], data: &mut [u8]) {
        // SAFETY: as in `encrypt_block`.
        unsafe { self.crypt_hw::<true>(prefix, ctr, y, data) }
    }

    pub(super) fn open(&self, prefix: &[u8; 12], ctr: u32, y: &mut [u64; 2], data: &mut [u8]) {
        // SAFETY: as in `encrypt_block`.
        unsafe { self.crypt_hw::<false>(prefix, ctr, y, data) }
    }

    #[target_feature(enable = "aes")]
    fn encrypt_block_hw(&self, b: [u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
        let mut out = [0u8; BLOCK_LEN];
        // SAFETY: both pointers address 16-byte arrays.
        unsafe {
            let c = self.aes.load().encrypt(ld(b.as_ptr()));
            vst1q_u8(out.as_mut_ptr(), c);
        }
        out
    }

    /// Hashes `N` blocks read from `src`.
    ///
    /// # Safety
    ///
    /// `src` must be valid for reading `16 * N` bytes.
    #[inline]
    #[target_feature(enable = "aes")]
    unsafe fn ghash_blocks<const N: usize>(
        p: &Powers,
        acc: uint64x2_t,
        src: *const u8,
    ) -> uint64x2_t {
        let mut x = [vdupq_n_u64(0); N];
        for (i, xi) in x.iter_mut().enumerate() {
            // SAFETY: block `i` lies inside the caller's `16 * N` bytes.
            *xi = natural(unsafe { ld(src.add(BLOCK_LEN * i)) });
        }
        p.hash(acc, &x)
    }

    #[target_feature(enable = "aes")]
    fn ghash_hw(&self, y: &mut [u64; 2], data: &[u8]) {
        let p = Powers::load(&self.h);
        // SAFETY: `y` is two `u64`s.
        let mut acc = unsafe { vld1q_u64(y.as_ptr()) };
        let (wide, rest) = data.as_chunks::<{ WIDE * BLOCK_LEN }>();
        for c in wide {
            // SAFETY: `c` is `16 * WIDE` bytes.
            acc = unsafe { Self::ghash_blocks::<WIDE>(&p, acc, c.as_ptr()) };
        }
        let (quads, rest) = rest.as_chunks::<{ 4 * BLOCK_LEN }>();
        for c in quads {
            // SAFETY: `c` is 64 bytes.
            acc = unsafe { Self::ghash_blocks::<4>(&p, acc, c.as_ptr()) };
        }
        for c in rest.chunks(BLOCK_LEN) {
            let mut block = [0u8; BLOCK_LEN];
            block[..c.len()].copy_from_slice(c);
            // SAFETY: `block` is 16 bytes.
            acc = unsafe { Self::ghash_blocks::<1>(&p, acc, block.as_ptr()) };
        }
        // SAFETY: `y` is two `u64`s.
        unsafe { vst1q_u64(y.as_mut_ptr(), acc) };
    }

    /// Encrypts (`SEAL`) or decrypts `N` blocks at `buf` in place with
    /// counters `ctr..ctr + N` and hashes the ciphertext.
    ///
    /// # Safety
    ///
    /// `buf` must be valid for reading and writing `16 * N` bytes.
    #[inline]
    #[target_feature(enable = "aes")]
    unsafe fn crypt_blocks<const N: usize, const SEAL: bool>(
        k: &Loaded,
        p: &Powers,
        base: uint32x4_t,
        ctr: u32,
        acc: uint64x2_t,
        buf: *mut u8,
    ) -> uint64x2_t {
        let mut ctrs = [vdupq_n_u8(0); N];
        for (i, b) in ctrs.iter_mut().enumerate() {
            *b = counter(base, ctr.wrapping_add(i as u32));
        }
        let ks = k.encrypt_n(ctrs);
        let mut ct = [vdupq_n_u64(0); N];
        for (i, (c, k)) in ct.iter_mut().zip(ks).enumerate() {
            // SAFETY: block `i` lies inside the caller's `16 * N` bytes.
            unsafe {
                let ptr = buf.add(BLOCK_LEN * i);
                let input = ld(ptr);
                let output = veorq_u8(input, k);
                *c = natural(if SEAL { output } else { input });
                vst1q_u8(ptr, output);
            }
        }
        p.hash(acc, &ct)
    }

    /// Counter-mode encryption (`SEAL`) or decryption fused with GHASH
    /// over the ciphertext.
    #[target_feature(enable = "aes")]
    fn crypt_hw<const SEAL: bool>(
        &self,
        prefix: &[u8; 12],
        mut ctr: u32,
        y: &mut [u64; 2],
        data: &mut [u8],
    ) {
        let k = self.aes.load();
        let p = Powers::load(&self.h);
        let base = counter_base(prefix);
        // SAFETY: `y` is two `u64`s.
        let mut acc = unsafe { vld1q_u64(y.as_ptr()) };
        let (wide, rest) = data.as_chunks_mut::<{ WIDE * BLOCK_LEN }>();
        for c in wide {
            // SAFETY: `c` is `16 * WIDE` bytes.
            acc =
                unsafe { Self::crypt_blocks::<WIDE, SEAL>(&k, &p, base, ctr, acc, c.as_mut_ptr()) };
            ctr = ctr.wrapping_add(WIDE as u32);
        }
        let (quads, rest) = rest.as_chunks_mut::<{ 4 * BLOCK_LEN }>();
        for c in quads {
            // SAFETY: `c` is 64 bytes.
            acc = unsafe { Self::crypt_blocks::<4, SEAL>(&k, &p, base, ctr, acc, c.as_mut_ptr()) };
            ctr = ctr.wrapping_add(4);
        }
        for c in rest.chunks_mut(BLOCK_LEN) {
            let mut block = [0u8; BLOCK_LEN];
            block[..c.len()].copy_from_slice(c);
            if SEAL && c.len() < BLOCK_LEN {
                // the hash covers the zero-padded ciphertext, so the
                // keystream past the end must not reach it
                let mut ks = [0u8; BLOCK_LEN];
                // SAFETY: `ks` is 16 bytes.
                unsafe { vst1q_u8(ks.as_mut_ptr(), k.encrypt(counter(base, ctr))) };
                for (b, k) in block[..c.len()].iter_mut().zip(ks) {
                    *b ^= k;
                }
                // SAFETY: `block` is 16 bytes.
                acc = unsafe { Self::ghash_blocks::<1>(&p, acc, block.as_ptr()) };
            } else {
                // SAFETY: `block` is 16 bytes.
                acc = unsafe {
                    Self::crypt_blocks::<1, SEAL>(&k, &p, base, ctr, acc, block.as_mut_ptr())
                };
            }
            ctr = ctr.wrapping_add(1);
            c.copy_from_slice(&block[..c.len()]);
        }
        // SAFETY: `y` is two `u64`s.
        unsafe { vst1q_u64(y.as_mut_ptr(), acc) };
    }
}
