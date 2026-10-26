//! AES encryption on the x86-64 AES-NI instructions: `aesenc` (one full
//! round) and `aesenclast` (the last round, without MixColumns), which run
//! in constant time and read no tables.
//!
//! Every function that executes the instructions carries
//! `#[target_feature(enable = "aes")]`. They are reached only through
//! [`gcm::x86_64`](crate::gcm), which builds its key only after the
//! caller's [`CpuFeatures`](crate::cpu::CpuFeatures) vouched for the
//! feature.
#![allow(unsafe_code)]

use core::arch::x86_64::*;

use super::{BLOCK_LEN, MAX_ROUNDS};
use crate::ct::zeroize::zeroize;

/// Round keys as bytes, in the order the instructions consume them,
/// 16-byte aligned so that no round-key load splits a cache line.
#[repr(C, align(16))]
pub(crate) struct Keys {
    rk: [[u8; BLOCK_LEN]; MAX_ROUNDS + 1],
    rounds: usize,
}

impl Keys {
    /// Takes the expanded key words (`4 * (rounds + 1)` of them) from
    /// [`expand_key`](super::expand_key).
    pub(crate) fn new(words: &[u32], rounds: usize) -> Self {
        let mut rk = [[0u8; BLOCK_LEN]; MAX_ROUNDS + 1];
        for (key, w) in rk.iter_mut().zip(words.as_chunks::<4>().0) {
            for (bytes, word) in key.as_chunks_mut::<4>().0.iter_mut().zip(w) {
                *bytes = word.to_le_bytes();
            }
        }
        Keys { rk, rounds }
    }

    pub(crate) fn wipe(&mut self) {
        zeroize(self.rk.as_flattened_mut());
    }

    /// Round key `k` in a vector register.
    #[inline]
    #[target_feature(enable = "aes")]
    fn ld(k: &[u8; BLOCK_LEN]) -> __m128i {
        // SAFETY: the pointer addresses a whole 16-byte round key; `loadu`
        // has no alignment requirement.
        unsafe { _mm_loadu_si128(k.as_ptr().cast()) }
    }

    #[inline]
    #[target_feature(enable = "aes")]
    pub(crate) fn encrypt(&self, b: __m128i) -> __m128i {
        let mut b = _mm_xor_si128(b, Self::ld(&self.rk[0]));
        for k in &self.rk[1..self.rounds] {
            b = _mm_aesenc_si128(b, Self::ld(k));
        }
        _mm_aesenclast_si128(b, Self::ld(&self.rk[self.rounds]))
    }

    /// `N` independent blocks, interleaved so the instruction latencies
    /// overlap.
    #[inline]
    #[target_feature(enable = "aes")]
    pub(crate) fn encrypt_n<const N: usize>(&self, mut b: [__m128i; N]) -> [__m128i; N] {
        let k = Self::ld(&self.rk[0]);
        for x in &mut b {
            *x = _mm_xor_si128(*x, k);
        }
        for k in &self.rk[1..self.rounds] {
            let k = Self::ld(k);
            for x in &mut b {
                *x = _mm_aesenc_si128(*x, k);
            }
        }
        let k = Self::ld(&self.rk[self.rounds]);
        for x in &mut b {
            *x = _mm_aesenclast_si128(*x, k);
        }
        b
    }
}
