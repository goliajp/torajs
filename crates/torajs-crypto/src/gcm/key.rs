//! The AES-GCM key types and the SP 800-38D construction over either
//! engine.
#![forbid(unsafe_code)]

use core::fmt;

use crate::aead::{Aead, AeadError, NONCE_LEN, TAG_LEN, sealed};
use crate::aes::{BLOCK_LEN, expand_key};
use crate::cpu::CpuFeatures;
use crate::ct::ct_eq;
use crate::ct::zeroize::zeroize;
use crate::dit;

#[cfg(all(
    target_arch = "aarch64",
    target_endian = "little",
    not(torajs_crypto_portable)
))]
use super::aarch64;
use super::soft;
#[cfg(all(
    target_arch = "x86_64",
    target_feature = "sse2",
    not(torajs_crypto_portable)
))]
use super::x86_64;

/// Longest plaintext: `2^39 - 256` bits (SP 800-38D §5.2.1.1).
const MAX_DATA_LEN: u64 = (1 << 36) - 32;
/// Longest AAD and IV: `2^64 - 1` bits.
const MAX_AUX_LEN: u64 = (1 << 61) - 1;

// both variants live inline in the key: the crate has no allocator to box
// the larger one with
#[allow(clippy::large_enum_variant)]
enum Engine {
    Soft(soft::Gcm),
    #[cfg(all(
        target_arch = "aarch64",
        target_endian = "little",
        not(torajs_crypto_portable)
    ))]
    Arm(aarch64::Gcm),
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "sse2",
        not(torajs_crypto_portable)
    ))]
    X86(x86_64::Gcm),
}

/// Dispatches one call to whichever engine the key was built with.
macro_rules! engine {
    ($self:expr, $e:ident => $body:expr) => {
        match &$self.engine {
            Engine::Soft($e) => $body,
            #[cfg(all(
                target_arch = "aarch64",
                target_endian = "little",
                not(torajs_crypto_portable)
            ))]
            Engine::Arm($e) => $body,
            #[cfg(all(
                target_arch = "x86_64",
                target_feature = "sse2",
                not(torajs_crypto_portable)
            ))]
            Engine::X86($e) => $body,
        }
    };
}

/// A keyed AES-GCM instance shared by the three public key types.
pub(super) struct Core {
    engine: Engine,
    dit: dit::Mode,
}

impl Drop for Core {
    fn drop(&mut self) {
        match &mut self.engine {
            Engine::Soft(e) => e.wipe(),
            #[cfg(all(
                target_arch = "aarch64",
                target_endian = "little",
                not(torajs_crypto_portable)
            ))]
            Engine::Arm(e) => e.wipe(),
            #[cfg(all(
                target_arch = "x86_64",
                target_feature = "sse2",
                not(torajs_crypto_portable)
            ))]
            Engine::X86(e) => e.wipe(),
        }
    }
}

/// The hardware engine `cpu` vouches for, or the portable one.
#[cfg_attr(
    not(any(
        all(
            target_arch = "aarch64",
            target_endian = "little",
            not(torajs_crypto_portable)
        ),
        all(
            target_arch = "x86_64",
            target_feature = "sse2",
            not(torajs_crypto_portable)
        )
    )),
    allow(unused_variables)
)]
fn pick(words: &[u32], rounds: usize, cpu: CpuFeatures) -> Engine {
    #[cfg(all(
        target_arch = "aarch64",
        target_endian = "little",
        not(torajs_crypto_portable)
    ))]
    if let Some(e) = aarch64::Gcm::new(words, rounds, cpu) {
        return Engine::Arm(e);
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "sse2",
        not(torajs_crypto_portable)
    ))]
    if let Some(e) = x86_64::Gcm::new(words, rounds, cpu) {
        return Engine::X86(e);
    }
    Engine::Soft(soft::Gcm::new(words, rounds))
}

impl Core {
    pub(super) fn new(key: &[u8], cpu: CpuFeatures) -> Core {
        let dit = dit::Mode::new(cpu);
        let _dit = dit.enter();
        let (mut words, rounds) = expand_key(key);
        let engine = pick(&words, rounds, cpu);
        zeroize(&mut words);
        Core { engine, dit }
    }

    #[cfg(test)]
    pub(super) fn is_hardware(&self) -> bool {
        !matches!(self.engine, Engine::Soft(_))
    }

    /// The pre-counter block `J0` (SP 800-38D §7.1 step 2).
    fn j0(&self, iv: &[u8]) -> [u8; BLOCK_LEN] {
        let mut j0 = [0u8; BLOCK_LEN];
        if iv.len() == NONCE_LEN {
            j0[..NONCE_LEN].copy_from_slice(iv);
            j0[15] = 1;
            return j0;
        }
        let mut y = [0u64; 2];
        let mut lens = [0u8; BLOCK_LEN];
        lens[8..].copy_from_slice(&(iv.len() as u64 * 8).to_be_bytes());
        engine!(self, e => {
            e.ghash(&mut y, iv);
            e.ghash(&mut y, &lens);
        });
        // a hashed J0 is a polynomial in H with known coefficients
        let j0 = soft::store(y);
        zeroize(&mut y);
        j0
    }

    /// Runs GCM over `data` and returns the tag; `SEAL` encrypts, otherwise
    /// decrypts. The hash always covers the ciphertext.
    fn crypt<const SEAL: bool>(&self, iv: &[u8], aad: &[u8], data: &mut [u8]) -> [u8; TAG_LEN] {
        let mut j0 = self.j0(iv);
        let mut prefix: [u8; 12] = j0[..12].try_into().unwrap();
        let ctr = u32::from_be_bytes(j0[12..].try_into().unwrap()).wrapping_add(1);
        let mut lens = [0u8; BLOCK_LEN];
        lens[..8].copy_from_slice(&(aad.len() as u64 * 8).to_be_bytes());
        lens[8..].copy_from_slice(&(data.len() as u64 * 8).to_be_bytes());
        let mut y = [0u64; 2];
        let mut mask = engine!(self, e => {
            e.ghash(&mut y, aad);
            if SEAL {
                e.seal(&prefix, ctr, &mut y, data);
            } else {
                e.open(&prefix, ctr, &mut y, data);
            }
            e.ghash(&mut y, &lens);
            e.encrypt_block(j0)
        });
        let mut tag = soft::store(y);
        for (t, m) in tag.iter_mut().zip(mask) {
            *t ^= m;
        }
        // the hash is a polynomial in H with public coefficients, so it
        // reveals the hash key; E_K(J0) is secret too, and so is J0 when it
        // was hashed from the IV
        zeroize(&mut y);
        zeroize(&mut mask);
        if iv.len() != NONCE_LEN {
            zeroize(&mut j0);
            zeroize(&mut prefix);
        }
        tag
    }

    fn lengths_ok(iv: &[u8], aad: &[u8], data: &[u8]) -> bool {
        !iv.is_empty()
            && iv.len() as u64 <= MAX_AUX_LEN
            && aad.len() as u64 <= MAX_AUX_LEN
            && data.len() as u64 <= MAX_DATA_LEN
    }

    pub(super) fn seal(
        &self,
        iv: &[u8],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<[u8; TAG_LEN], AeadError> {
        if !Self::lengths_ok(iv, aad, in_out) {
            return Err(AeadError);
        }
        let _dit = self.dit.enter();
        Ok(self.crypt::<true>(iv, aad, in_out))
    }

    pub(super) fn open(
        &self,
        iv: &[u8],
        aad: &[u8],
        in_out: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<(), AeadError> {
        if !Self::lengths_ok(iv, aad, in_out) {
            zeroize(in_out);
            return Err(AeadError);
        }
        let _dit = self.dit.enter();
        // decryption is fused with hashing, so the plaintext exists in the
        // buffer before the verdict; a mismatch wipes it
        let mut ours = self.crypt::<false>(iv, aad, in_out);
        let ok = ct_eq(&ours, tag).reveal();
        zeroize(&mut ours);
        if ok {
            Ok(())
        } else {
            zeroize(in_out);
            Err(AeadError)
        }
    }
}

macro_rules! aes_gcm {
    ($name:ident, $len:literal, $bits:literal) => {
        #[doc = concat!("An AES-", $bits, "-GCM key.")]
        ///
        /// The round keys and the hash key are wiped on drop; `Debug`
        /// prints the type name only.
        pub struct $name(Core);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(stringify!($name))
            }
        }

        impl $name {
            #[doc = concat!("Expands a ", $bits, "-bit key. `cpu` selects the")]
            /// hardware path when it vouches for the AES and PMULL
            /// instructions; [`CpuFeatures::portable`] forces the portable
            /// code.
            pub fn new(key: &[u8; $len], cpu: CpuFeatures) -> Self {
                $name(Core::new(key, cpu))
            }

            /// [`Aead::seal`] with an IV of any non-zero length. A
            /// 12-byte IV gives the same result as [`Aead::seal`]; other
            /// lengths are hashed into the initial counter (SP 800-38D §7.1).
            ///
            /// # Errors
            ///
            /// [`AeadError`] when `iv` is empty or a length exceeds the
            /// SP 800-38D limits; `in_out` is left unchanged then.
            pub fn seal_with_iv(
                &self,
                iv: &[u8],
                aad: &[u8],
                in_out: &mut [u8],
            ) -> Result<[u8; TAG_LEN], AeadError> {
                self.0.seal(iv, aad, in_out)
            }

            /// [`Aead::open`] with an IV of any non-zero length.
            ///
            /// # Errors
            ///
            /// [`AeadError`] when the tag does not match, `iv` is empty or
            /// a length exceeds the limits; `in_out` is overwritten with
            /// zeros then.
            pub fn open_with_iv(
                &self,
                iv: &[u8],
                aad: &[u8],
                in_out: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> Result<(), AeadError> {
                self.0.open(iv, aad, in_out, tag)
            }
        }

        impl sealed::Sealed for $name {}

        impl Aead for $name {
            const KEY_LEN: usize = $len;

            fn seal(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                in_out: &mut [u8],
            ) -> Result<[u8; TAG_LEN], AeadError> {
                self.0.seal(nonce, aad, in_out)
            }

            fn open(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                in_out: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> Result<(), AeadError> {
                self.0.open(nonce, aad, in_out, tag)
            }
        }
    };
}

aes_gcm!(Aes128Gcm, 16, "128");
aes_gcm!(Aes192Gcm, 24, "192");
aes_gcm!(Aes256Gcm, 32, "256");
