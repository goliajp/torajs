//! The entropy interface.
//!
//! This crate never reads a random source itself. Functions that need
//! randomness take a `&mut impl EntropySource` supplied by the caller,
//! which decides where the bytes come from (`getentropy`, `getrandom`,
//! a hardware generator, a fixed sequence in tests) and what to do when
//! that source fails.
//!
//! ```
//! use torajs_crypto::rand::{EntropyError, EntropySource};
//!
//! /// A deterministic source for tests. Never use it for real keys.
//! struct Counter(u8);
//!
//! impl EntropySource for Counter {
//!     fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
//!         for b in buf {
//!             self.0 = self.0.wrapping_add(1);
//!             *b = self.0;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let mut src = Counter(0);
//! let mut nonce = [0u8; 4];
//! src.fill(&mut nonce).unwrap();
//! assert_eq!(nonce, [1, 2, 3, 4]);
//! ```
#![forbid(unsafe_code)]

use core::fmt;

/// The entropy source could not produce the requested bytes.
///
/// When this is returned the contents of the buffer are unspecified and
/// must not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntropyError;

impl fmt::Display for EntropyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("entropy source failed")
    }
}

impl core::error::Error for EntropyError {}

/// A source of cryptographically secure random bytes.
pub trait EntropySource {
    /// Fills all of `buf` with random bytes.
    ///
    /// # Errors
    ///
    /// [`EntropyError`] when the source cannot deliver `buf.len()` bytes.
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError>;
}

impl<T: EntropySource + ?Sized> EntropySource for &mut T {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError> {
        (**self).fill(buf)
    }
}
