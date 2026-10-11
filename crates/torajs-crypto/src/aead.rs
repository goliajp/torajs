//! Authenticated encryption with associated data.
//!
//! [`Aead`] is implemented by
//! [`ChaCha20Poly1305`](crate::chacha20poly1305::ChaCha20Poly1305)
//! (RFC 8439) and by the AES-GCM key types in [`gcm`](crate::gcm)
//! (NIST SP 800-38D). All of them take a 96-bit nonce and produce a
//! 128-bit tag, and all of them work in place on a buffer the caller owns.
//!
//! ```
//! use torajs_crypto::Aead;
//! use torajs_crypto::chacha20poly1305::ChaCha20Poly1305;
//! use torajs_crypto::cpu::CpuFeatures;
//!
//! let key = ChaCha20Poly1305::new(&[7; 32], CpuFeatures::compile_time());
//! let nonce = [1; 12];
//! let mut buf = *b"attack at dawn";
//! let tag = key.seal(&nonce, b"header", &mut buf).unwrap();
//! assert_ne!(&buf, b"attack at dawn");
//! key.open(&nonce, b"header", &mut buf, &tag).unwrap();
//! assert_eq!(&buf, b"attack at dawn");
//! ```
#![forbid(unsafe_code)]

use core::fmt;

/// Nonce length in bytes (96 bits).
pub const NONCE_LEN: usize = 12;

/// Tag length in bytes (128 bits).
pub const TAG_LEN: usize = 16;

/// Sealing or opening failed.
///
/// `open` returns it when the tag does not match; `seal` and `open` return
/// it when a length is outside what the algorithm allows. The error
/// carries no detail on purpose: it should not tell a caller, or an
/// attacker watching the caller, which check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AeadError;

impl fmt::Display for AeadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AEAD operation failed")
    }
}

impl core::error::Error for AeadError {}

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// An AEAD key: seals and opens messages in place.
///
/// The trait is sealed; only the key types of this crate implement it.
pub trait Aead: fmt::Debug + sealed::Sealed {
    /// Key length in bytes.
    const KEY_LEN: usize;

    /// Encrypts `in_out` in place and returns the tag over `aad` and the
    /// ciphertext.
    ///
    /// A nonce must never be used twice with the same key.
    ///
    /// # Errors
    ///
    /// [`AeadError`] when `in_out` or `aad` is longer than the algorithm
    /// allows; `in_out` is left unchanged then.
    fn seal(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<[u8; TAG_LEN], AeadError>;

    /// Checks `tag` against `aad` and the ciphertext in `in_out`, and on
    /// success decrypts `in_out` in place. The tag comparison runs in
    /// constant time.
    ///
    /// # Errors
    ///
    /// [`AeadError`] when the tag does not match or a length is out of
    /// range. `in_out` is then overwritten with zeros, so no
    /// unauthenticated plaintext reaches the caller.
    fn open(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        in_out: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<(), AeadError>;
}
