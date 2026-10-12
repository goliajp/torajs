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
    /// AArch64 `FEAT_AdvSIMD` (NEON): the 128-bit vector instructions.
    Aarch64Neon,
    /// x86-64 AES-NI (CPUID.1:ECX bit 25).
    X86Aes,
    /// x86-64 PCLMULQDQ (CPUID.1:ECX bit 1).
    X86Pclmulqdq,
    /// x86-64 SHA extensions (CPUID.(EAX=7,ECX=0):EBX bit 29).
    X86Sha,
    /// AArch64 `FEAT_DIT`: the `PSTATE.DIT` bit, data-independent timing
    /// (see [`with_data_independent_timing`]).
    Aarch64Dit,
    /// AArch64 `FEAT_SB`: the `sb` speculation barrier.
    Aarch64Sb,
    /// x86-64 SSSE3 (CPUID.1:ECX bit 9): `pshufb` and the other byte
    /// shuffles.
    X86Ssse3,
}

const ALL: [Feature; 11] = [
    Feature::Aarch64Aes,
    Feature::Aarch64Pmull,
    Feature::Aarch64Sha2,
    Feature::Aarch64Sha512,
    Feature::Aarch64Neon,
    Feature::X86Aes,
    Feature::X86Pclmulqdq,
    Feature::X86Sha,
    Feature::Aarch64Dit,
    Feature::Aarch64Sb,
    Feature::X86Ssse3,
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
        if cfg!(all(target_arch = "aarch64", target_feature = "neon")) {
            bits |= Feature::Aarch64Neon.bit();
        }
        if cfg!(all(target_arch = "aarch64", target_feature = "dit")) {
            bits |= Feature::Aarch64Dit.bit();
        }
        if cfg!(all(target_arch = "aarch64", target_feature = "sb")) {
            bits |= Feature::Aarch64Sb.bit();
        }
        if cfg!(all(target_arch = "x86_64", target_feature = "ssse3")) {
            bits |= Feature::X86Ssse3.bit();
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

/// Runs `f` with data-independent timing switched on, when `cpu` vouches
/// for [`Feature::Aarch64Dit`], and restores the previous state when `f`
/// returns or unwinds.
///
/// On AArch64 this sets `PSTATE.DIT`, under which the instructions the
/// architecture lists for `FEAT_DIT` take a time independent of their
/// operand values, followed by a speculation barrier (`sb` when `cpu`
/// vouches for [`Feature::Aarch64Sb`], `dsb nsh; isb` otherwise). The bit is per
/// thread. A scope entered with the bit already set leaves it set, so
/// scopes nest. Without the feature, or on other architectures, `f` simply
/// runs.
///
/// The AEAD keys of this crate already wrap key setup, `seal` and `open`
/// in this scope when built with such a `cpu`; use it around other code
/// that handles secrets, such as HMAC or HKDF.
///
/// Writing the bit is not free: on an Apple M4 Pro one switch on and off
/// costs about 40 ns, more than a whole 64-byte AES-GCM seal. A caller
/// that seals or opens many small records in a row can wrap the whole run
/// in one scope; the per-call scopes inside then find the bit set and
/// only read it.
///
/// ```
/// use torajs_crypto::cpu::{CpuFeatures, with_data_independent_timing};
/// use torajs_crypto::hmac::Hmac;
/// use torajs_crypto::sha2::Sha256;
///
/// let tag = with_data_independent_timing(CpuFeatures::compile_time(), || {
///     Hmac::<Sha256>::mac(b"key", b"message")
/// });
/// assert_eq!(tag[0], 0x6e);
/// ```
pub fn with_data_independent_timing<R>(cpu: CpuFeatures, f: impl FnOnce() -> R) -> R {
    let _dit = crate::dit::Mode::new(cpu).enter();
    f()
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
        assert_eq!(
            c.has(Feature::Aarch64Neon),
            cfg!(all(target_arch = "aarch64", target_feature = "neon"))
        );
    }
}
