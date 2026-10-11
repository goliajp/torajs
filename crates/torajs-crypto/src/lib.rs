//! Dependency-free cryptographic primitives for `no_std` targets.
//!
//! The crate has no dependencies, never allocates, never reads a clock,
//! a random source or a CPU-feature register on its own. Every external
//! effect is passed in by the caller:
//!
//! - randomness through an [`EntropySource`](rand::EntropySource),
//! - CPU capabilities through a [`CpuFeatures`](cpu::CpuFeatures) value,
//!   which the AES-GCM constructors take to choose between the hardware
//!   and the portable code (the SHA-256 ARMv8 path is still chosen at
//!   compile time).
//!
//! # Modules
//!
//! | module | contents |
//! |---|---|
//! | [`ct`] | constant-time comparison and selection, [`ct::zeroize`] for wiping secrets |
//! | [`sha2`] | SHA-256, SHA-384, SHA-512 (FIPS 180-4) behind the [`Digest`] trait |
//! | [`hmac`] | HMAC (RFC 2104) over any [`Digest`] |
//! | [`hkdf`] | HKDF extract / expand (RFC 5869) over any [`Digest`] |
//! | [`aead`] | the [`Aead`] trait, [`AeadError`](aead::AeadError), nonce and tag lengths |
//! | [`gcm`] | AES-128 / 192 / 256-GCM (NIST SP 800-38D) |
//! | [`chacha20poly1305`] | ChaCha20-Poly1305 (RFC 8439) |
//! | [`rand`] | the [`EntropySource`](rand::EntropySource) trait callers implement |
//! | [`cpu`] | [`CpuFeatures`](cpu::CpuFeatures), the hardware capabilities a caller vouches for |
//!
//! # Example
//!
//! ```
//! use torajs_crypto::hkdf::Hkdf;
//! use torajs_crypto::hmac::Hmac;
//! use torajs_crypto::sha2::{Sha256, sha256};
//!
//! let digest = sha256(b"abc");
//! assert_eq!(digest[0], 0xba);
//!
//! let tag = Hmac::<Sha256>::mac(b"key", b"message");
//! assert!(Hmac::<Sha256>::new(b"key").chain(b"message").verify(&tag).is_ok());
//!
//! let prk = Hkdf::<Sha256>::extract(b"salt", b"input keying material");
//! let mut okm = [0u8; 42];
//! prk.expand(b"context", &mut okm).unwrap();
//! ```
//!
//! # Side-channel rules
//!
//! These rules hold for every function in the crate that touches a secret
//! (keys, MAC states, derived secrets, plaintext, keystream, Poly1305 and
//! GHASH accumulators):
//!
//! 1. A secret never decides a branch, a loop bound, an array index or a
//!    divisor, and is never compared with an early-exit comparison.
//! 2. No secret indexes a table: AES is bitsliced or runs on the AES
//!    instructions, GHASH multiplies with integer or PMULL instructions.
//! 3. Secret comparisons and selections go through [`ct`]. A [`ct::Choice`]
//!    passes through [`core::hint::black_box`] so the optimizer is less
//!    likely to turn a mask select back into a branch; this lowers the odds
//!    rather than guaranteeing anything, so the compiled code is audited at
//!    the instruction level as well.
//! 4. Lengths are public. Input and output lengths may decide control flow.
//! 5. Key material owned by a type of this crate is wiped with
//!    [`ct::zeroize`] when the value is dropped.
//!
//! Public data (hash inputs, signatures, public keys) has no constant-time
//! requirement. Power analysis and speculative-execution attacks are out of
//! scope.
//!
//! # `unsafe`
//!
//! The crate root denies `unsafe_code` and every module that does not use
//! hardware intrinsics forbids it. Unsafe code exists in [`ct::zeroize`]
//! (volatile writes) and in the AArch64 hardware modules: the SHA-256 block
//! function, compiled only when the target enables the `sha2` feature at
//! build time, and the AES and GHASH code, reached only through a
//! [`CpuFeatures`](cpu::CpuFeatures) value that vouches for the AES and
//! PMULL instructions.
//! [`cpu::CpuFeatures::with`] is an `unsafe fn` as a caller contract: the
//! caller vouches that the CPU has the feature.
//!
//! Building with `--cfg torajs_crypto_portable` disables every hardware
//! path, so the portable code can be tested on any machine.

#![no_std]
#![deny(unsafe_code)]

#[cfg(test)]
extern crate std;

pub mod aead;
mod aes;
mod chacha20;
pub mod chacha20poly1305;
pub mod cpu;
pub mod ct;
mod digest;
pub mod gcm;
pub mod hkdf;
pub mod hmac;
mod poly1305;
pub mod rand;
pub mod sha2;
#[cfg(test)]
mod testutil;

pub use aead::Aead;
pub use digest::Digest;
