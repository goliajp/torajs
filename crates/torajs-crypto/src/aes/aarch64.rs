//! AES encryption on the ARMv8 AES instructions: `aese` (AddRoundKey,
//! SubBytes, ShiftRows) and `aesmc` (MixColumns), which run in constant
//! time and read no tables.
//!
//! Every function that executes the instructions carries
//! `#[target_feature(enable = "aes")]`. They are reached only through
//! [`gcm::aarch64`](crate::gcm), which builds its key only after the
//! caller's [`CpuFeatures`](crate::cpu::CpuFeatures) vouched for the
//! feature.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::{BLOCK_LEN, MAX_ROUNDS};
use crate::ct::zeroize::zeroize;

/// Round keys as bytes, in the order the instructions consume them.
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

    /// Loads the round keys into vector registers.
    #[inline]
    #[target_feature(enable = "aes")]
    pub(crate) fn load(&self) -> Loaded {
        let mut rk = [vdupq_n_u8(0); MAX_ROUNDS + 1];
        for (r, k) in rk.iter_mut().zip(&self.rk) {
            // SAFETY: the pointer addresses a whole 16-byte round key.
            *r = unsafe { vld1q_u8(k.as_ptr()) };
        }
        Loaded {
            rk,
            rounds: self.rounds,
        }
    }
}

/// Round keys held in registers for the length of one operation.
pub(crate) struct Loaded {
    rk: [uint8x16_t; MAX_ROUNDS + 1],
    rounds: usize,
}

impl Loaded {
    #[inline]
    #[target_feature(enable = "aes")]
    pub(crate) fn encrypt(&self, mut b: uint8x16_t) -> uint8x16_t {
        let last = self.rounds - 1;
        for &k in &self.rk[..last] {
            b = vaesmcq_u8(vaeseq_u8(b, k));
        }
        veorq_u8(vaeseq_u8(b, self.rk[last]), self.rk[self.rounds])
    }

    /// `N` independent blocks, interleaved so the instruction latencies
    /// overlap.
    #[inline]
    #[target_feature(enable = "aes")]
    pub(crate) fn encrypt_n<const N: usize>(&self, mut b: [uint8x16_t; N]) -> [uint8x16_t; N] {
        let last = self.rounds - 1;
        for &k in &self.rk[..last] {
            for x in &mut b {
                *x = vaesmcq_u8(vaeseq_u8(*x, k));
            }
        }
        for x in &mut b {
            *x = veorq_u8(vaeseq_u8(*x, self.rk[last]), self.rk[self.rounds]);
        }
        b
    }
}
