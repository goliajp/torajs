//! ChaCha20-Poly1305 AEAD (RFC 8439 §2.8).
//!
//! The constructor takes a [`CpuFeatures`]: on AArch64, when it vouches
//! for [`Feature::Aarch64Neon`](crate::cpu::Feature::Aarch64Neon), the
//! keystream runs on NEON registers, three groups of four blocks
//! interleaved; otherwise on portable integer code. Both produce identical
//! output. When it vouches for
//! [`Feature::Aarch64Dit`](crate::cpu::Feature::Aarch64Dit), key setup,
//! `seal` and `open` run with data-independent timing switched on (see
//! [`with_data_independent_timing`](crate::cpu::with_data_independent_timing)).
//!
//! ```
//! use torajs_crypto::Aead;
//! use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;
//! use torajs_crypto::cpu::CpuFeatures;
//!
//! let key = ChaCha20Poly1305::new(&[0x42; 32], CpuFeatures::compile_time());
//! let mut record = *b"GET / HTTP/1.1";
//! let tag = key.seal(&[0; 12], b"", &mut record).unwrap();
//! assert!(key.open(&[0; 12], b"", &mut record, &tag).is_ok());
//! assert_eq!(&record, b"GET / HTTP/1.1");
//! ```
#![forbid(unsafe_code)]

use core::fmt;

use crate::aead::{Aead, AeadError, NONCE_LEN, TAG_LEN, sealed};
use crate::chacha20::{self, BLOCK_LEN, Engine};
use crate::cpu::CpuFeatures;
use crate::ct::ct_eq;
use crate::ct::zeroize::zeroize;
use crate::dit;
use crate::poly1305::{self, Poly1305};

/// Longest plaintext: the 32-bit block counter starts at 1 (RFC 8439 §2.8).
const MAX_DATA_LEN: u64 = ((1 << 32) - 1) * BLOCK_LEN as u64;

/// A ChaCha20-Poly1305 key. Wiped on drop; `Debug` prints the type name
/// only.
pub struct ChaCha20Poly1305 {
    key: [u32; 8],
    engine: Engine,
    dit: dit::Mode,
}

impl fmt::Debug for ChaCha20Poly1305 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChaCha20Poly1305")
    }
}

impl Drop for ChaCha20Poly1305 {
    fn drop(&mut self) {
        zeroize(&mut self.key);
    }
}

impl ChaCha20Poly1305 {
    /// Builds a key from 32 bytes. `cpu` selects the keystream code (see
    /// the [module documentation](self)).
    pub fn new(key: &[u8; 32], cpu: CpuFeatures) -> Self {
        let dit = dit::Mode::new(cpu);
        let _dit = dit.enter();
        ChaCha20Poly1305 {
            key: chacha20::key_words(key),
            engine: Engine::new(cpu),
            dit,
        }
    }

    /// The tag over `aad` and `ciphertext` (RFC 8439 §2.8): Poly1305 keyed
    /// with the first 32 bytes of keystream block 0.
    fn tag(&self, nonce: &[u32; 3], aad: &[u8], ciphertext: &[u8]) -> [u8; TAG_LEN] {
        let mut block0 = chacha20::block(&self.key, 0, nonce);
        let mut otk = [0u8; poly1305::KEY_LEN];
        for (bytes, w) in otk.as_chunks_mut::<4>().0.iter_mut().zip(&block0) {
            *bytes = w.to_le_bytes();
        }
        zeroize(&mut block0);
        let mut mac = Poly1305::new(&otk);
        zeroize(&mut otk);
        mac.update_padded(aad);
        mac.update_padded(ciphertext);
        let mut lens = [0u8; 16];
        lens[..8].copy_from_slice(&(aad.len() as u64).to_le_bytes());
        lens[8..].copy_from_slice(&(ciphertext.len() as u64).to_le_bytes());
        mac.update(&lens);
        mac.finish()
    }
}

impl sealed::Sealed for ChaCha20Poly1305 {}

impl Aead for ChaCha20Poly1305 {
    const KEY_LEN: usize = 32;

    fn seal(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<[u8; TAG_LEN], AeadError> {
        if in_out.len() as u64 > MAX_DATA_LEN {
            return Err(AeadError);
        }
        let _dit = self.dit.enter();
        let nonce = chacha20::nonce_words(nonce);
        self.engine.xor_keystream(&self.key, 1, &nonce, in_out);
        Ok(self.tag(&nonce, aad, in_out))
    }

    fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        in_out: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<(), AeadError> {
        if in_out.len() as u64 > MAX_DATA_LEN {
            zeroize(in_out);
            return Err(AeadError);
        }
        let _dit = self.dit.enter();
        let nonce = chacha20::nonce_words(nonce);
        // authenticate before decrypting: a forged message never turns
        // into plaintext, and the buffer is wiped on failure anyway
        let mut ours = self.tag(&nonce, aad, in_out);
        let ok = ct_eq(&ours, tag).reveal();
        zeroize(&mut ours);
        if !ok {
            zeroize(in_out);
            return Err(AeadError);
        }
        self.engine.xor_keystream(&self.key, 1, &nonce, in_out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{hex, unhex};

    const PLAINTEXT: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you \
only one tip for the future, sunscreen would be it.";

    const CPUS: [CpuFeatures; 2] = [CpuFeatures::portable(), CpuFeatures::compile_time()];

    fn rfc_key(cpu: CpuFeatures) -> ChaCha20Poly1305 {
        let key: [u8; 32] = core::array::from_fn(|i| 0x80 + i as u8);
        ChaCha20Poly1305::new(&key, cpu)
    }

    /// RFC 8439 §2.8.2.
    #[test]
    fn rfc8439_aead_example() {
        let nonce: [u8; 12] = unhex("070000004041424344454647").try_into().unwrap();
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        for cpu in CPUS {
            let mut buf = PLAINTEXT.to_vec();
            let tag = rfc_key(cpu).seal(&nonce, &aad, &mut buf).unwrap();
            assert_eq!(
                hex(&buf),
                "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6\
                 3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36\
                 92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc\
                 3ff4def08e4b7a9de576d26586cec64b6116"
            );
            assert_eq!(hex(&tag), "1ae10b594f09e26a7e902ecbd0600691");
            rfc_key(cpu).open(&nonce, &aad, &mut buf, &tag).unwrap();
            assert_eq!(buf, PLAINTEXT);
        }
    }

    #[test]
    fn open_rejects_any_flipped_bit_and_wipes() {
        let key = rfc_key(CpuFeatures::compile_time());
        let nonce = [9u8; 12];
        let mut sealed = PLAINTEXT[..40].to_vec();
        let tag = key.seal(&nonce, b"aad", &mut sealed).unwrap();
        for i in 0..sealed.len() * 8 {
            let mut buf = sealed.clone();
            buf[i / 8] ^= 1 << (i % 8);
            assert_eq!(key.open(&nonce, b"aad", &mut buf, &tag), Err(AeadError));
            assert!(buf.iter().all(|&b| b == 0), "bit {i}");
        }
        for i in 0..TAG_LEN * 8 {
            let mut bad = tag;
            bad[i / 8] ^= 1 << (i % 8);
            let mut buf = sealed.clone();
            assert!(key.open(&nonce, b"aad", &mut buf, &bad).is_err());
        }
        let mut buf = sealed.clone();
        assert!(key.open(&nonce, b"aae", &mut buf, &tag).is_err());
    }

    #[test]
    fn debug_hides_the_key() {
        assert_eq!(
            std::format!("{:?}", rfc_key(CpuFeatures::compile_time())),
            "ChaCha20Poly1305"
        );
    }
}
