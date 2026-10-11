//! HKDF (RFC 5869) over any [`Digest`].
//!
//! [`Hkdf::extract`] turns input keying material into a pseudorandom key;
//! [`Hkdf::expand`] derives output keying material from it. TLS 1.3's
//! `HKDF-Expand-Label` is built on [`Hkdf::expand_parts`], which takes the
//! `info` string in pieces so the caller does not need a buffer for it.
//!
//! ```
//! use torajs_crypto::hkdf::Hkdf;
//! use torajs_crypto::sha2::Sha256;
//!
//! let prk = Hkdf::<Sha256>::extract(b"salt", b"secret");
//! let mut key = [0u8; 16];
//! prk.expand(b"handshake key", &mut key).unwrap();
//! ```
#![forbid(unsafe_code)]

use core::fmt;

use crate::Digest;
use crate::ct::zeroize::zeroize;
use crate::hmac::Hmac;

/// The requested length is not allowed: an output longer than
/// `255 * OUTPUT_LEN` bytes, or a pseudorandom key whose length is not
/// `OUTPUT_LEN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLength;

impl fmt::Display for InvalidLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid HKDF length")
    }
}

impl core::error::Error for InvalidLength {}

/// A pseudorandom key (RFC 5869 §2.2 `PRK`), ready for expansion. Wiped on
/// drop.
#[derive(Clone)]
pub struct Hkdf<D: Digest> {
    prk: D::Output,
}

impl<D: Digest> fmt::Debug for Hkdf<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Hkdf")
    }
}

impl<D: Digest> Drop for Hkdf<D> {
    fn drop(&mut self) {
        zeroize(self.prk.as_mut());
    }
}

impl<D: Digest> Hkdf<D> {
    /// `HKDF-Extract(salt, IKM)` (RFC 5869 §2.2). An empty `salt` is the
    /// same as `OUTPUT_LEN` zero bytes.
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Self {
        Hkdf {
            prk: Hmac::<D>::mac(salt, ikm),
        }
    }

    /// Wraps an existing pseudorandom key, for example a TLS 1.3 secret
    /// that is used directly as a PRK.
    ///
    /// # Errors
    ///
    /// [`InvalidLength`] when `prk.len() != OUTPUT_LEN`.
    pub fn from_prk(prk: &[u8]) -> Result<Self, InvalidLength> {
        if prk.len() != D::OUTPUT_LEN {
            return Err(InvalidLength);
        }
        let mut out = D::zero_output();
        out.as_mut().copy_from_slice(prk);
        Ok(Hkdf { prk: out })
    }

    /// The pseudorandom key bytes.
    pub fn prk(&self) -> &[u8] {
        self.prk.as_ref()
    }

    /// `HKDF-Expand(PRK, info, L)` (RFC 5869 §2.3) with `L = okm.len()`.
    ///
    /// # Errors
    ///
    /// [`InvalidLength`] when `okm.len() > 255 * OUTPUT_LEN`; `okm` is left
    /// untouched.
    pub fn expand(&self, info: &[u8], okm: &mut [u8]) -> Result<(), InvalidLength> {
        self.expand_parts(&[info], okm)
    }

    /// Like [`expand`](Self::expand), with `info` given as the
    /// concatenation of `info_parts`.
    ///
    /// # Errors
    ///
    /// [`InvalidLength`] when `okm.len() > 255 * OUTPUT_LEN`.
    pub fn expand_parts(&self, info_parts: &[&[u8]], okm: &mut [u8]) -> Result<(), InvalidLength> {
        if okm.len() > 255 * D::OUTPUT_LEN {
            return Err(InvalidLength);
        }
        let mut t = D::zero_output();
        let mut t_len = 0;
        for (i, chunk) in okm.chunks_mut(D::OUTPUT_LEN).enumerate() {
            let mut mac = Hmac::<D>::new(self.prk.as_ref());
            mac.update(&t.as_ref()[..t_len]);
            for part in info_parts {
                mac.update(part);
            }
            // at most 255 blocks, checked above
            mac.update(&[(i + 1) as u8]);
            t = mac.finish();
            t_len = D::OUTPUT_LEN;
            chunk.copy_from_slice(&t.as_ref()[..chunk.len()]);
        }
        zeroize(t.as_mut());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::{Sha256, Sha384};
    use crate::testutil::{hex, unhex};
    use std::vec;
    use std::vec::Vec;

    fn run(ikm: &[u8], salt: &[u8], info: &[u8], len: usize, prk: &str, okm: &str) {
        let h = Hkdf::<Sha256>::extract(salt, ikm);
        assert_eq!(hex(h.prk()), prk);
        let mut out = vec![0u8; len];
        h.expand(info, &mut out).unwrap();
        assert_eq!(hex(&out), okm);
        let again = Hkdf::<Sha256>::from_prk(&unhex(prk)).unwrap();
        let mut split = vec![0u8; len];
        let mid = info.len() / 2;
        again
            .expand_parts(&[&info[..mid], &[], &info[mid..]], &mut split)
            .unwrap();
        assert_eq!(split, out);
    }

    /// RFC 5869 Appendix A.1.
    #[test]
    fn rfc5869_case1() {
        run(
            &[0x0b; 22],
            &unhex("000102030405060708090a0b0c"),
            &unhex("f0f1f2f3f4f5f6f7f8f9"),
            42,
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5",
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865",
        );
    }

    /// RFC 5869 Appendix A.2.
    #[test]
    fn rfc5869_case2() {
        let ikm: Vec<u8> = (0x00..=0x4f).collect();
        let salt: Vec<u8> = (0x60..=0xaf).collect();
        let info: Vec<u8> = (0xb0..=0xff).collect();
        run(
            &ikm,
            &salt,
            &info,
            82,
            "06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244",
            "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
             59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
             cc30c58179ec3e87c14c01d5c1f3434f1d87",
        );
    }

    /// RFC 5869 Appendix A.3.
    #[test]
    fn rfc5869_case3() {
        run(
            &[0x0b; 22],
            &[],
            &[],
            42,
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04",
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8",
        );
    }

    #[test]
    fn length_limits() {
        let h = Hkdf::<Sha384>::extract(b"s", b"k");
        let mut max = vec![0u8; 255 * 48];
        assert!(h.expand(b"", &mut max).is_ok());
        let mut over = vec![7u8; 255 * 48 + 1];
        assert_eq!(h.expand(b"", &mut over), Err(InvalidLength));
        assert!(over.iter().all(|&b| b == 7));
        let mut empty = [0u8; 0];
        assert!(h.expand(b"", &mut empty).is_ok());
        assert_eq!(
            Hkdf::<Sha384>::from_prk(&[0; 32]).err(),
            Some(InvalidLength)
        );
        assert_eq!(std::format!("{h:?}"), "Hkdf");
    }
}
