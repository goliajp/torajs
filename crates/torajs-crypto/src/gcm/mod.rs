//! AES-GCM (NIST SP 800-38D) with AES-128, AES-192 and AES-256.
//!
//! The constructors take a [`CpuFeatures`](crate::cpu::CpuFeatures). On
//! AArch64, when it vouches for both
//! [`Feature::Aarch64Aes`](crate::cpu::Feature::Aarch64Aes) and
//! [`Feature::Aarch64Pmull`](crate::cpu::Feature::Aarch64Pmull), the key
//! uses the AES and PMULL instructions; on x86-64, when it vouches for
//! [`Feature::X86Aes`](crate::cpu::Feature::X86Aes),
//! [`Feature::X86Pclmulqdq`](crate::cpu::Feature::X86Pclmulqdq) and
//! [`Feature::X86Ssse3`](crate::cpu::Feature::X86Ssse3), the AES-NI and
//! PCLMULQDQ instructions (on a target built with SSE2; a target without
//! it, such as `x86_64-unknown-none`, keeps the portable code); otherwise
//! it uses the portable
//! code: bitsliced AES and a GHASH built from integer multiplications,
//! neither of which reads a table. Both produce identical output. When it
//! vouches for [`Feature::Aarch64Dit`](crate::cpu::Feature::Aarch64Dit),
//! key setup, sealing and opening run with data-independent timing
//! switched on (see
//! [`with_data_independent_timing`](crate::cpu::with_data_independent_timing)).
//!
//! The [`Aead`](crate::Aead) methods take the 96-bit nonce every protocol
//! uses. [`Aes128Gcm::seal_with_iv`] and [`Aes128Gcm::open_with_iv`] (and
//! their counterparts on the other key types) accept an IV of any non-zero
//! length, as SP 800-38D and WebCrypto allow.
//!
//! ```
//! use torajs_crypto::Aead;
//! use torajs_crypto::cpu::CpuFeatures;
//! use torajs_crypto::gcm::Aes128Gcm;
//!
//! let key = Aes128Gcm::new(&[0; 16], CpuFeatures::compile_time());
//! let mut buf = [0u8; 16];
//! let tag = key.seal(&[0; 12], b"", &mut buf).unwrap();
//! // SP 800-38D test case 2 (McGrew and Viega)
//! assert_eq!(buf[..4], [0x03, 0x88, 0xda, 0xce]);
//! assert_eq!(tag[..4], [0xab, 0x6e, 0x47, 0xd4]);
//! ```

#[cfg(all(
    target_arch = "aarch64",
    target_endian = "little",
    not(torajs_crypto_portable)
))]
mod aarch64;
mod key;
mod soft;
#[cfg(test)]
mod tests;
#[cfg(all(
    target_arch = "x86_64",
    target_feature = "sse2",
    not(torajs_crypto_portable)
))]
mod x86_64;

pub use key::{Aes128Gcm, Aes192Gcm, Aes256Gcm};
