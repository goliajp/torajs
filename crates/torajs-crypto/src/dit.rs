//! Arm data-independent timing (`FEAT_DIT`, `PSTATE.DIT`).
//!
//! With `PSTATE.DIT` set, the instructions the Arm architecture lists for
//! the feature (integer and vector arithmetic, logic, shifts,
//! multiplications, conditional selects, loads and stores, the AES and
//! PMULL instructions) take a time that does not depend on the data they
//! operate on. Without it a core may finish some of them early on some
//! operand values. Apple asks constant-time code to set the bit around
//! every operation on secrets, followed by a speculation barrier, and to
//! restore the previous value afterwards so nested scopes compose; its own
//! cryptographic libraries do so.
//!
//! The bit is per thread and is saved across context switches. It is
//! written only when the caller's [`CpuFeatures`] vouches for
//! [`Feature::Aarch64Dit`]; the barrier is `sb` when it also vouches for
//! [`Feature::Aarch64Sb`], `dsb nsh; isb` otherwise.
#![allow(unsafe_code)]

use crate::cpu::CpuFeatures;
#[cfg(target_arch = "aarch64")]
use crate::cpu::Feature;

/// Whether a key sets `PSTATE.DIT` around its operations, chosen once from
/// the caller's [`CpuFeatures`].
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mode {
    Off,
    #[cfg(target_arch = "aarch64")]
    Dit {
        sb: bool,
    },
}

impl Mode {
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_variables))]
    pub(crate) const fn new(cpu: CpuFeatures) -> Mode {
        #[cfg(target_arch = "aarch64")]
        if cpu.has(Feature::Aarch64Dit) {
            return Mode::Dit {
                sb: cpu.has(Feature::Aarch64Sb),
            };
        }
        Mode::Off
    }

    /// Sets `PSTATE.DIT` until the returned guard is dropped.
    #[inline]
    pub(crate) fn enter(self) -> Guard {
        match self {
            Mode::Off => Guard { set: false },
            #[cfg(target_arch = "aarch64")]
            Mode::Dit { sb } => {
                // SAFETY: a `Mode::Dit` is built only from a `CpuFeatures`
                // that vouches for FEAT_DIT, and with `sb` set only when it
                // also vouches for FEAT_SB (the contract of
                // `CpuFeatures::with`).
                let was_on = unsafe { hw::read() };
                if !was_on {
                    // SAFETY: as above.
                    unsafe {
                        hw::set();
                        if sb {
                            hw::sb();
                        } else {
                            hw::dsb_isb();
                        }
                    }
                }
                Guard { set: !was_on }
            }
        }
    }
}

/// Clears `PSTATE.DIT` on drop if [`Mode::enter`] set it, so a scope
/// entered with the bit already on leaves it on.
#[derive(Debug)]
#[must_use]
pub(crate) struct Guard {
    #[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
    set: bool,
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(target_arch = "aarch64")]
        if self.set {
            // SAFETY: `set` is true only after `Mode::enter` wrote the bit,
            // which it does only on a CPU with FEAT_DIT.
            unsafe { hw::clear() };
        }
    }
}

/// The register accesses. None of them is marked `nomem`: each acts as a
/// compiler barrier, so loads of secret data are not moved across the
/// switch.
#[cfg(target_arch = "aarch64")]
mod hw {
    use core::arch::asm;

    /// Bit 24 of the `DIT` system register mirrors `PSTATE.DIT`.
    const DIT_BIT: u64 = 1 << 24;

    #[inline]
    #[target_feature(enable = "dit")]
    pub(super) unsafe fn read() -> bool {
        let v: u64;
        // SAFETY: the caller guarantees FEAT_DIT, which defines the register.
        unsafe { asm!("mrs {}, DIT", out(reg) v, options(nostack, preserves_flags)) };
        v & DIT_BIT != 0
    }

    #[inline]
    #[target_feature(enable = "dit")]
    pub(super) unsafe fn set() {
        // SAFETY: as in `read`.
        unsafe { asm!("msr DIT, #1", options(nostack, preserves_flags)) };
    }

    #[inline]
    #[target_feature(enable = "dit")]
    pub(super) unsafe fn clear() {
        // SAFETY: as in `read`.
        unsafe { asm!("msr DIT, #0", options(nostack, preserves_flags)) };
    }

    #[inline]
    #[target_feature(enable = "sb")]
    pub(super) unsafe fn sb() {
        // SAFETY: the caller guarantees FEAT_SB.
        unsafe { asm!("sb", options(nostack, preserves_flags)) };
    }

    /// The barrier where `sb` is not vouched for: `dsb nsh; isb`, both in
    /// the base architecture, completes the write and holds back the
    /// instructions after it until the new `PSTATE.DIT` is in effect.
    #[inline]
    pub(super) unsafe fn dsb_isb() {
        // SAFETY: `dsb` and `isb` exist on every AArch64 CPU.
        unsafe { asm!("dsb nsh", "isb", options(nostack, preserves_flags)) };
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::*;

    fn detected() -> Option<CpuFeatures> {
        if !std::arch::is_aarch64_feature_detected!("dit") {
            return None;
        }
        // SAFETY: FEAT_DIT was just detected.
        let mut cpu = unsafe { CpuFeatures::portable().with(Feature::Aarch64Dit) };
        if std::arch::is_aarch64_feature_detected!("sb") {
            // SAFETY: FEAT_SB was just detected.
            cpu = unsafe { cpu.with(Feature::Aarch64Sb) };
        }
        Some(cpu)
    }

    fn is_on() -> bool {
        // SAFETY: only called after `detected` found FEAT_DIT.
        unsafe { hw::read() }
    }

    #[test]
    fn scopes_set_and_restore_the_bit() {
        let Some(cpu) = detected() else { return };
        let mode = Mode::new(cpu);
        assert!(!is_on());
        {
            let _outer = mode.enter();
            assert!(is_on());
            {
                let _inner = mode.enter();
                assert!(is_on());
            }
            // the inner scope found the bit on and left it on
            assert!(is_on());
        }
        assert!(!is_on());
        let _off = Mode::new(CpuFeatures::portable()).enter();
        assert!(!is_on());
    }
}
