//! GCM on the x86-64 AES-NI and PCLMULQDQ instructions.
//!
//! The arithmetic is that of the AArch64 path: field elements in the
//! natural polynomial order of [`soft`](super::soft), reached by reversing
//! the bits of each byte of a GHASH block (two `pshufb` lookups of the
//! nibbles in a register-held table, so no lookup touches memory), 64x64
//! `pclmulqdq` products that need no shifting, and a reduction that folds
//! the top two 64-bit limbs with two more products by
//! `x^7 + x^2 + x + 1`. Up to eight blocks are hashed per reduction with
//! the precomputed powers `H^1..H^8`, and counter-mode encryption of the
//! same blocks is interleaved with the hashing.
//!
//! A [`Gcm`] is only constructed when the caller's [`CpuFeatures`] has
//! [`Feature::X86Aes`], [`Feature::X86Pclmulqdq`] and
//! [`Feature::X86Ssse3`]; that is the invariant every `// SAFETY:` comment
//! below relies on. The module is compiled only for targets whose ABI has
//! SSE2; soft-float targets such as `x86_64-unknown-none` keep no vector
//! state and take the portable code.
#![allow(unsafe_code)]

use core::arch::x86_64::*;

use crate::aes::BLOCK_LEN;
use crate::aes::x86_64::Keys;
use crate::cpu::{CpuFeatures, Feature};
use crate::ct::zeroize::zeroize;

/// Blocks per GHASH reduction on the main path.
const WIDE: usize = 8;

/// Keyed GCM state: round keys, `H^1..H^8` in natural order, and for
/// each power the XOR of its halves in the low lane for the Karatsuba
/// middle product. 16-byte aligned so that no vector load splits a cache
/// line.
#[repr(C, align(16))]
pub(super) struct Gcm {
    aes: Keys,
    h: [[u64; 2]; WIDE],
    fold: [[u64; 2]; WIDE],
}

#[inline]
#[target_feature(enable = "pclmulqdq")]
fn clmul_lo(a: __m128i, b: __m128i) -> __m128i {
    _mm_clmulepi64_si128::<0x00>(a, b)
}

#[inline]
#[target_feature(enable = "pclmulqdq")]
fn clmul_hi(a: __m128i, b: __m128i) -> __m128i {
    _mm_clmulepi64_si128::<0x11>(a, b)
}

/// The XOR of the two halves, in the low lane.
#[inline]
#[target_feature(enable = "sse2")]
fn fold(a: __m128i) -> __m128i {
    _mm_xor_si128(a, _mm_shuffle_epi32::<0x4e>(a))
}

/// `pshufb` tables: a nibble with its four bits reversed, placed in the
/// high and in the low half of a byte.
const REV_TO_HI: [u8; 16] = [
    0x00, 0x80, 0x40, 0xc0, 0x20, 0xa0, 0x60, 0xe0, 0x10, 0x90, 0x50, 0xd0, 0x30, 0xb0, 0x70, 0xf0,
];
const REV_TO_LO: [u8; 16] = [
    0x0, 0x8, 0x4, 0xc, 0x2, 0xa, 0x6, 0xe, 0x1, 0x9, 0x5, 0xd, 0x3, 0xb, 0x7, 0xf,
];

/// A GHASH block in natural order: the bits of every byte reversed.
#[inline]
#[target_feature(enable = "ssse3")]
fn natural(b: __m128i) -> __m128i {
    // SAFETY: both tables are 16 bytes; `loadu` has no alignment
    // requirement.
    let (to_hi, to_lo) = unsafe {
        (
            _mm_loadu_si128(REV_TO_HI.as_ptr().cast()),
            _mm_loadu_si128(REV_TO_LO.as_ptr().cast()),
        )
    };
    let mask = _mm_set1_epi8(0x0f);
    let lo = _mm_and_si128(b, mask);
    let hi = _mm_and_si128(_mm_srli_epi16::<4>(b), mask);
    _mm_or_si128(_mm_shuffle_epi8(to_hi, lo), _mm_shuffle_epi8(to_lo, hi))
}

/// Unreduced sum of products: low, Karatsuba middle and high 128 bits.
#[derive(Clone, Copy)]
struct Acc {
    lo: __m128i,
    mid: __m128i,
    hi: __m128i,
}

impl Acc {
    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    fn new(x: __m128i, h: __m128i, hf: __m128i) -> Acc {
        Acc {
            lo: clmul_lo(x, h),
            mid: clmul_lo(fold(x), hf),
            hi: clmul_hi(x, h),
        }
    }

    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    fn add(&mut self, x: __m128i, h: __m128i, hf: __m128i) {
        self.lo = _mm_xor_si128(self.lo, clmul_lo(x, h));
        self.mid = _mm_xor_si128(self.mid, clmul_lo(fold(x), hf));
        self.hi = _mm_xor_si128(self.hi, clmul_hi(x, h));
    }

    /// Combines the three products into 256 bits and reduces modulo
    /// `x^128 + x^7 + x^2 + x + 1`.
    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    fn reduce(self) -> __m128i {
        let mid = _mm_xor_si128(self.mid, _mm_xor_si128(self.lo, self.hi));
        let lo = _mm_xor_si128(self.lo, _mm_slli_si128::<8>(mid));
        let hi = _mm_xor_si128(self.hi, _mm_srli_si128::<8>(mid));
        let g = _mm_set_epi64x(0, 0x87);
        // x^192 = x^64 (x^7 + x^2 + x + 1)
        let t = _mm_clmulepi64_si128::<0x01>(hi, g);
        let lo = _mm_xor_si128(lo, _mm_slli_si128::<8>(t));
        let hi = _mm_xor_si128(hi, _mm_srli_si128::<8>(t));
        // x^128 = x^7 + x^2 + x + 1
        let u = _mm_clmulepi64_si128::<0x00>(hi, g);
        _mm_xor_si128(lo, u)
    }
}

/// Loads two `u64`s.
#[inline]
#[target_feature(enable = "sse2")]
fn ld64(p: &[u64; 2]) -> __m128i {
    // SAFETY: `p` is 16 bytes; `loadu` has no alignment requirement.
    unsafe { _mm_loadu_si128(p.as_ptr().cast()) }
}

/// Stores two `u64`s.
#[inline]
#[target_feature(enable = "sse2")]
fn st64(p: &mut [u64; 2], v: __m128i) {
    // SAFETY: `p` is 16 bytes; `storeu` has no alignment requirement.
    unsafe { _mm_storeu_si128(p.as_mut_ptr().cast(), v) }
}

/// Loads 16 bytes from `p`.
///
/// # Safety
///
/// `p` must be valid for reading 16 bytes.
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn ld(p: *const u8) -> __m128i {
    // SAFETY: forwarded from the caller; `loadu` has no alignment
    // requirement.
    unsafe { _mm_loadu_si128(p.cast()) }
}

/// Stores 16 bytes to `p`.
///
/// # Safety
///
/// `p` must be valid for writing 16 bytes.
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn st(p: *mut u8, v: __m128i) {
    // SAFETY: forwarded from the caller.
    unsafe { _mm_storeu_si128(p.cast(), v) }
}

/// The counter block: `base` holds the 12-byte prefix and zeros, the last
/// four bytes take `ctr` big-endian.
#[inline]
#[target_feature(enable = "sse2")]
fn counter(base: __m128i, ctr: u32) -> __m128i {
    _mm_or_si128(base, _mm_set_epi32(ctr.to_be() as i32, 0, 0, 0))
}

#[inline]
#[target_feature(enable = "sse2")]
fn counter_base(prefix: &[u8; 12]) -> __m128i {
    let mut b = [0u8; BLOCK_LEN];
    b[..12].copy_from_slice(prefix);
    // SAFETY: `b` is 16 bytes.
    unsafe { ld(b.as_ptr()) }
}

impl Gcm {
    /// Builds the hardware state, or `None` when `cpu` does not vouch for
    /// AES-NI, PCLMULQDQ and SSSE3.
    pub(super) fn new(words: &[u32], rounds: usize, cpu: CpuFeatures) -> Option<Self> {
        if !(cpu.has(Feature::X86Aes)
            && cpu.has(Feature::X86Pclmulqdq)
            && cpu.has(Feature::X86Ssse3))
        {
            return None;
        }
        let mut gcm = Gcm {
            aes: Keys::new(&words[..4 * (rounds + 1)], rounds),
            h: [[0; 2]; WIDE],
            fold: [[0; 2]; WIDE],
        };
        // SAFETY: `cpu` vouches for AES-NI, PCLMULQDQ and SSSE3 (the
        // contract of `CpuFeatures::with`), which is all `init_powers`
        // executes.
        unsafe { gcm.init_powers() };
        Some(gcm)
    }

    pub(super) fn wipe(&mut self) {
        self.aes.wipe();
        zeroize(self.h.as_flattened_mut());
        zeroize(self.fold.as_flattened_mut());
    }

    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    fn init_powers(&mut self) {
        let h = natural(self.aes.encrypt(_mm_setzero_si128()));
        let hf = fold(h);
        let mut p = h;
        for (slot, fslot) in self.h.iter_mut().zip(&mut self.fold) {
            st64(slot, p);
            st64(fslot, fold(p));
            p = Acc::new(p, h, hf).reduce();
        }
    }

    /// `(y ^ x[0]) * H^N ^ x[1] * H^(N-1) ^ ... ^ x[N-1] * H`, one
    /// reduction.
    #[inline]
    #[target_feature(enable = "pclmulqdq")]
    fn hash<const N: usize>(&self, y: __m128i, x: &[__m128i; N]) -> __m128i {
        let pow = |i: usize| (ld64(&self.h[i]), ld64(&self.fold[i]));
        let (h, hf) = pow(N - 1);
        let mut acc = Acc::new(_mm_xor_si128(y, x[0]), h, hf);
        for (i, &xi) in x.iter().enumerate().skip(1) {
            let (h, hf) = pow(N - 1 - i);
            acc.add(xi, h, hf);
        }
        acc.reduce()
    }

    pub(super) fn encrypt_block(&self, b: [u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
        // SAFETY: a `Gcm` exists only when the CPU has AES-NI, PCLMULQDQ
        // and SSSE3 (see `new`).
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

    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    fn encrypt_block_hw(&self, b: [u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
        let mut out = [0u8; BLOCK_LEN];
        // SAFETY: both pointers address 16-byte arrays.
        unsafe {
            let c = self.aes.encrypt(ld(b.as_ptr()));
            st(out.as_mut_ptr(), c);
        }
        out
    }

    /// Hashes `N` blocks read from `src`.
    ///
    /// # Safety
    ///
    /// `src` must be valid for reading `16 * N` bytes.
    #[inline]
    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    unsafe fn ghash_blocks<const N: usize>(&self, acc: __m128i, src: *const u8) -> __m128i {
        let mut x = [_mm_setzero_si128(); N];
        for (i, xi) in x.iter_mut().enumerate() {
            // SAFETY: block `i` lies inside the caller's `16 * N` bytes.
            *xi = natural(unsafe { ld(src.add(BLOCK_LEN * i)) });
        }
        self.hash(acc, &x)
    }

    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    fn ghash_hw(&self, y: &mut [u64; 2], data: &[u8]) {
        let mut acc = ld64(y);
        let (wide, rest) = data.as_chunks::<{ WIDE * BLOCK_LEN }>();
        for c in wide {
            // SAFETY: `c` is `16 * WIDE` bytes.
            acc = unsafe { self.ghash_blocks::<WIDE>(acc, c.as_ptr()) };
        }
        let (quads, rest) = rest.as_chunks::<{ 4 * BLOCK_LEN }>();
        for c in quads {
            // SAFETY: `c` is 64 bytes.
            acc = unsafe { self.ghash_blocks::<4>(acc, c.as_ptr()) };
        }
        for c in rest.chunks(BLOCK_LEN) {
            let mut block = [0u8; BLOCK_LEN];
            block[..c.len()].copy_from_slice(c);
            // SAFETY: `block` is 16 bytes.
            acc = unsafe { self.ghash_blocks::<1>(acc, block.as_ptr()) };
        }
        st64(y, acc);
    }

    /// Encrypts (`SEAL`) or decrypts `N` blocks at `buf` in place with
    /// counters `ctr..ctr + N` and hashes the ciphertext.
    ///
    /// # Safety
    ///
    /// `buf` must be valid for reading and writing `16 * N` bytes.
    #[inline]
    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    unsafe fn crypt_blocks<const N: usize, const SEAL: bool>(
        &self,
        base: __m128i,
        ctr: u32,
        acc: __m128i,
        buf: *mut u8,
    ) -> __m128i {
        let mut ctrs = [_mm_setzero_si128(); N];
        for (i, b) in ctrs.iter_mut().enumerate() {
            *b = counter(base, ctr.wrapping_add(i as u32));
        }
        let ks = self.aes.encrypt_n(ctrs);
        let mut ct = [_mm_setzero_si128(); N];
        for (i, (c, k)) in ct.iter_mut().zip(ks).enumerate() {
            // SAFETY: block `i` lies inside the caller's `16 * N` bytes.
            unsafe {
                let ptr = buf.add(BLOCK_LEN * i);
                let input = ld(ptr);
                let output = _mm_xor_si128(input, k);
                *c = natural(if SEAL { output } else { input });
                st(ptr, output);
            }
        }
        self.hash(acc, &ct)
    }

    /// Counter-mode encryption (`SEAL`) or decryption fused with GHASH
    /// over the ciphertext.
    #[target_feature(enable = "aes,pclmulqdq,sse2,ssse3")]
    fn crypt_hw<const SEAL: bool>(
        &self,
        prefix: &[u8; 12],
        mut ctr: u32,
        y: &mut [u64; 2],
        data: &mut [u8],
    ) {
        let base = counter_base(prefix);
        let mut acc = ld64(y);
        let (wide, rest) = data.as_chunks_mut::<{ WIDE * BLOCK_LEN }>();
        for c in wide {
            // SAFETY: `c` is `16 * WIDE` bytes.
            acc = unsafe { self.crypt_blocks::<WIDE, SEAL>(base, ctr, acc, c.as_mut_ptr()) };
            ctr = ctr.wrapping_add(WIDE as u32);
        }
        let (quads, rest) = rest.as_chunks_mut::<{ 4 * BLOCK_LEN }>();
        for c in quads {
            // SAFETY: `c` is 64 bytes.
            acc = unsafe { self.crypt_blocks::<4, SEAL>(base, ctr, acc, c.as_mut_ptr()) };
            ctr = ctr.wrapping_add(4);
        }
        for c in rest.chunks_mut(BLOCK_LEN) {
            // a stack copy of plaintext, wiped below
            let mut block = [0u8; BLOCK_LEN];
            block[..c.len()].copy_from_slice(c);
            if SEAL && c.len() < BLOCK_LEN {
                // the hash covers the zero-padded ciphertext, so the
                // keystream past the end must not reach it
                let mut ks = [0u8; BLOCK_LEN];
                // SAFETY: `ks` is 16 bytes.
                unsafe { st(ks.as_mut_ptr(), self.aes.encrypt(counter(base, ctr))) };
                for (b, k) in block[..c.len()].iter_mut().zip(ks) {
                    *b ^= k;
                }
                zeroize(&mut ks);
                // SAFETY: `block` is 16 bytes.
                acc = unsafe { self.ghash_blocks::<1>(acc, block.as_ptr()) };
            } else {
                // SAFETY: `block` is 16 bytes.
                acc = unsafe { self.crypt_blocks::<1, SEAL>(base, ctr, acc, block.as_mut_ptr()) };
            }
            ctr = ctr.wrapping_add(1);
            c.copy_from_slice(&block[..c.len()]);
            zeroize(&mut block);
        }
        st64(y, acc);
    }
}
