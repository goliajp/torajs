//! CPU capabilities that select hardware code paths.
//!
//! This crate does not probe the CPU. The caller detects the features
//! (with `cpuid`, `mrs`, `getauxval`, `sysctl`, ...) and passes the result
//! in as a [`CpuFeatures`]. A value claiming a feature the CPU lacks would
//! make a hardware path execute an unsupported instruction, so building
//! one with feature bits set is `unsafe`; [`CpuFeatures::portable`] and
//! [`CpuFeatures::compile_time`] are safe.
//!
//! ```
//! use torajs_crypto::cpu::{CpuFeatures, Feature};
//!
//! let none = CpuFeatures::portable();
//! assert!(!none.has(Feature::Aarch64Aes));
//!
//! // SAFETY: example only; a real caller sets a bit after detecting it.
//! let claimed = unsafe { CpuFeatures::portable().with(Feature::X86Aes) };
//! assert!(claimed.has(Feature::X86Aes));
//! ```
//!
//! `unsafe` in this module is limited to the declaration of
//! [`CpuFeatures::with`]; the crate-wide `deny(unsafe_code)` is lifted for
//! that one item only.

use core::fmt;

/// One hardware capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Feature {
    /// AArch64 `FEAT_AES`: AESE / AESD / AESMC / AESIMC.
    Aarch64Aes,
    /// AArch64 `FEAT_PMULL`: 64x64 polynomial multiply.
    Aarch64Pmull,
    /// AArch64 `FEAT_SHA256`: SHA256H / SHA256H2 / SHA256SU0 / SHA256SU1.
    Aarch64Sha2,
    /// AArch64 `FEAT_SHA512`: SHA512H / SHA512H2 / SHA512SU0 / SHA512SU1.
    Aarch64Sha512,
    /// x86-64 AES-NI (CPUID.1:ECX bit 25).
    X86Aes,
    /// x86-64 PCLMULQDQ (CPUID.1:ECX bit 1).
    X86Pclmulqdq,
    /// x86-64 SHA extensions (CPUID.(EAX=7,ECX=0):EBX bit 29).
    X86Sha,
}

const ALL: [Feature; 7] = [
    Feature::Aarch64Aes,
    Feature::Aarch64Pmull,
    Feature::Aarch64Sha2,
    Feature::Aarch64Sha512,
    Feature::X86Aes,
    Feature::X86Pclmulqdq,
    Feature::X86Sha,
];

impl Feature {
    const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// A set of CPU features the caller vouches for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CpuFeatures {
    bits: u32,
}

impl CpuFeatures {
    /// No hardware features: every primitive takes its portable path.
    pub const fn portable() -> Self {
        CpuFeatures { bits: 0 }
    }

    /// The features the target enables at build time
    /// (`cfg(target_feature)`). Code compiled this way only runs on CPUs
    /// that have them, so no runtime detection is needed.
    pub const fn compile_time() -> Self {
        let mut bits = 0;
        if cfg!(all(target_arch = "aarch64", target_feature = "aes")) {
            bits |= Feature::Aarch64Aes.bit() | Feature::Aarch64Pmull.bit();
        }
        if cfg!(all(target_arch = "aarch64", target_feature = "sha2")) {
            bits |= Feature::Aarch64Sha2.bit();
        }
        if cfg!(all(target_arch = "aarch64", target_feature = "sha3")) {
            bits |= Feature::Aarch64Sha512.bit();
        }
        if cfg!(all(target_arch = "x86_64", target_feature = "aes")) {
            bits |= Feature::X86Aes.bit();
        }
        if cfg!(all(target_arch = "x86_64", target_feature = "pclmulqdq")) {
            bits |= Feature::X86Pclmulqdq.bit();
        }
        if cfg!(all(target_arch = "x86_64", target_feature = "sha")) {
            bits |= Feature::X86Sha.bit();
        }
        CpuFeatures { bits }
    }

    /// Returns a copy with `feature` added.
    ///
    /// # Safety
    ///
    /// The CPU this value will be used on must support `feature`.
    #[must_use]
    #[allow(unsafe_code)]
    pub const unsafe fn with(self, feature: Feature) -> Self {
        CpuFeatures {
            bits: self.bits | feature.bit(),
        }
    }

    /// Returns a copy with `feature` removed. Always safe: dropping a
    /// feature only steers code to a slower path.
    #[must_use]
    pub const fn without(self, feature: Feature) -> Self {
        CpuFeatures {
            bits: self.bits & !feature.bit(),
        }
    }

    /// Whether `feature` is in the set.
    pub const fn has(self, feature: Feature) -> bool {
        self.bits & feature.bit() != 0
    }
}

impl fmt::Debug for CpuFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(ALL.iter().filter(|&&x| self.has(x)))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unsafe_code)]
    fn set_operations() {
        let p = CpuFeatures::portable();
        assert_eq!(p, CpuFeatures::default());
        assert!(ALL.iter().all(|&f| !p.has(f)));
        // SAFETY: the value is only inspected, never used to pick a path.
        let s = unsafe { p.with(Feature::Aarch64Aes).with(Feature::X86Sha) };
        assert!(s.has(Feature::Aarch64Aes) && s.has(Feature::X86Sha));
        assert!(!s.has(Feature::X86Aes));
        assert_eq!(s.without(Feature::X86Sha).without(Feature::Aarch64Aes), p);
        assert_eq!(std::format!("{s:?}"), "{Aarch64Aes, X86Sha}");
    }

    #[test]
    fn compile_time_matches_cfg() {
        let c = CpuFeatures::compile_time();
        assert_eq!(
            c.has(Feature::Aarch64Sha2),
            cfg!(all(target_arch = "aarch64", target_feature = "sha2"))
        );
    }
}
