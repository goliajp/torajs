//! SHA-256, SHA-384 and SHA-512 (FIPS 180-4).
//!
//! The streaming types implement [`Digest`](crate::Digest); [`sha256`],
//! [`sha384`] and [`sha512`] hash a whole message in one call.
//!
//! On AArch64 targets built with the `sha2` feature enabled (for example
//! `aarch64-apple-darwin`), the SHA-256 block function uses the ARMv8
//! SHA-256 instructions. Everything else uses the portable FIPS round
//! functions.

#[cfg(all(
    target_arch = "aarch64",
    target_feature = "sha2",
    not(torajs_crypto_portable)
))]
mod aarch64;
mod sha256;
mod sha512;

pub use sha256::{Sha256, sha256};
pub use sha512::{Sha384, Sha512, sha384, sha512};
