#![forbid(unsafe_code)]

use core::fmt;

pub(crate) mod sealed {
    pub trait Sealed {
        /// Overwrites the internal state with zeros. Used when the state
        /// was keyed (HMAC) and must not outlive its owner.
        fn wipe(&mut self);
    }
}

/// A streaming hash function with a fixed-size output.
///
/// Implemented by [`Sha256`](crate::sha2::Sha256),
/// [`Sha384`](crate::sha2::Sha384) and [`Sha512`](crate::sha2::Sha512).
/// The trait is sealed: [`hmac`](crate::hmac) and [`hkdf`](crate::hkdf)
/// rely on the block size and wiping behavior of exactly these types.
pub trait Digest: Clone + Default + fmt::Debug + sealed::Sealed {
    /// Output length in bytes.
    const OUTPUT_LEN: usize;
    /// Block length in bytes (the HMAC key-padding width).
    const BLOCK_LEN: usize;
    /// The digest, a byte array of length [`Self::OUTPUT_LEN`].
    type Output: Copy + AsRef<[u8]> + AsMut<[u8]> + Eq + fmt::Debug;

    /// An all-zero output value.
    #[doc(hidden)]
    fn zero_output() -> Self::Output;

    /// Starts a new hash computation.
    fn new() -> Self;

    /// Absorbs `data`.
    fn update(&mut self, data: &[u8]);

    /// Pads the message and returns the digest.
    fn finish(self) -> Self::Output;

    /// Absorbs `data` and returns the state, for call chains.
    #[must_use]
    fn chain(mut self, data: &[u8]) -> Self {
        self.update(data);
        self
    }

    /// Hashes `data` in one call.
    fn digest(data: &[u8]) -> Self::Output {
        Self::new().chain(data).finish()
    }
}
