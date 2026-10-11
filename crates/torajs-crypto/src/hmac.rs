//! HMAC (RFC 2104, FIPS 198-1) over any [`Digest`].
//!
//! ```
//! use torajs_crypto::hmac::Hmac;
//! use torajs_crypto::sha2::Sha384;
//!
//! let mut mac = Hmac::<Sha384>::new(b"key");
//! mac.update(b"part one, ");
//! mac.update(b"part two");
//! let tag = mac.finish();
//! assert_eq!(tag, Hmac::<Sha384>::mac(b"key", b"part one, part two"));
//! ```
#![forbid(unsafe_code)]

use core::fmt;

use crate::Digest;
use crate::ct::ct_eq;
use crate::ct::zeroize::zeroize;

/// Largest block length among the supported digests (SHA-512).
const MAX_BLOCK_LEN: usize = 128;

/// The tag did not match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacError;

impl fmt::Display for MacError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MAC verification failed")
    }
}

impl core::error::Error for MacError {}

/// A keyed HMAC computation. The key-derived states are wiped on drop.
#[derive(Clone)]
pub struct Hmac<D: Digest> {
    inner: D,
    outer: D,
}

impl<D: Digest> fmt::Debug for Hmac<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Hmac")
    }
}

impl<D: Digest> Drop for Hmac<D> {
    fn drop(&mut self) {
        self.inner.wipe();
        self.outer.wipe();
    }
}

impl<D: Digest> Hmac<D> {
    /// Starts an HMAC computation with `key`. Keys longer than the block
    /// length are hashed first, as RFC 2104 §2 specifies; any length is
    /// accepted.
    pub fn new(key: &[u8]) -> Self {
        let mut block = [0u8; MAX_BLOCK_LEN];
        let block = &mut block[..D::BLOCK_LEN];
        if key.len() > D::BLOCK_LEN {
            let mut hashed = D::digest(key);
            block[..D::OUTPUT_LEN].copy_from_slice(hashed.as_ref());
            zeroize(hashed.as_mut());
        } else {
            block[..key.len()].copy_from_slice(key);
        }

        for b in block.iter_mut() {
            *b ^= 0x36;
        }
        let inner = D::new().chain(block);
        for b in block.iter_mut() {
            *b ^= 0x36 ^ 0x5c;
        }
        let outer = D::new().chain(block);
        zeroize(block);
        Hmac { inner, outer }
    }

    /// Absorbs `data`.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Absorbs `data` and returns the state, for call chains.
    #[must_use]
    pub fn chain(mut self, data: &[u8]) -> Self {
        self.update(data);
        self
    }

    /// Returns the tag.
    pub fn finish(self) -> D::Output {
        let mut inner_hash = self.inner.clone().finish();
        let tag = self.outer.clone().chain(inner_hash.as_ref()).finish();
        zeroize(inner_hash.as_mut());
        tag
    }

    /// Checks `tag` against the full-length tag in constant time.
    pub fn verify(self, tag: &[u8]) -> Result<(), MacError> {
        let mut ours = self.finish();
        let ok = ct_eq(ours.as_ref(), tag).reveal();
        zeroize(ours.as_mut());
        if ok { Ok(()) } else { Err(MacError) }
    }

    /// Checks a truncated `tag` against the leftmost `tag.len()` bytes of
    /// the tag in constant time.
    ///
    /// RFC 2104 §5 recommends keeping at least half of the output and no
    /// fewer than 80 bits, so tags shorter than
    /// `max(OUTPUT_LEN / 2, 10)` bytes, and tags longer than the output, are
    /// rejected without computing anything.
    pub fn verify_truncated(self, tag: &[u8]) -> Result<(), MacError> {
        let min = (D::OUTPUT_LEN / 2).max(10);
        if tag.len() < min || tag.len() > D::OUTPUT_LEN {
            return Err(MacError);
        }
        let mut ours = self.finish();
        let ok = ct_eq(&ours.as_ref()[..tag.len()], tag).reveal();
        zeroize(ours.as_mut());
        if ok { Ok(()) } else { Err(MacError) }
    }

    /// Computes the tag of `data` under `key` in one call.
    pub fn mac(key: &[u8], data: &[u8]) -> D::Output {
        Self::new(key).chain(data).finish()
    }
}

#[cfg(test)]
mod tests;
