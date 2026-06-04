//! AArch64 register file.
//!
//! Modeled per ARM ARM DDI 0487 §C1.2 + Apple's ARM64 Function Calling
//! Convention. We track GPRs and FPRs separately because the
//! instruction encoding namespaces them distinctly (Xn/Wn for ints,
//! Vn/Dn/Sn for floats); register #31 is overloaded as either SP or
//! XZR depending on instruction context — we model SP as its own
//! sentinel variant so the encoder can be context-explicit.

/// General-purpose 64-bit registers `x0..x30` (31 entries) + `sp` /
/// `xzr` sentinels.
///
/// Instruction-encoding index is `as u8` for `X0..X30`. Use `Self::SP`
/// or `Self::XZR` when the instruction context needs the #31 slot —
/// callers must know which (e.g. ADD encodes #31 as SP if `sf=1,
/// shift=00, imm12=0` else XZR; see ARM ARM C5.1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum Gpr {
    X0 = 0,
    X1 = 1,
    X2 = 2,
    X3 = 3,
    X4 = 4,
    X5 = 5,
    X6 = 6,
    X7 = 7,
    X8 = 8,
    X9 = 9,
    X10 = 10,
    X11 = 11,
    X12 = 12,
    X13 = 13,
    X14 = 14,
    X15 = 15,
    X16 = 16,
    X17 = 17,
    /// Reserved on Apple platforms (Apple ARM64 ABI §3.2.6).
    X18 = 18,
    X19 = 19,
    X20 = 20,
    X21 = 21,
    X22 = 22,
    X23 = 23,
    X24 = 24,
    X25 = 25,
    X26 = 26,
    X27 = 27,
    X28 = 28,
    /// Frame pointer per AAPCS64.
    X29 = 29,
    /// Link register per AAPCS64.
    X30 = 30,
    /// Stack pointer (encoded as register #31 in SP-context instructions).
    SP = 31,
    /// Zero register (encoded as register #31 in non-SP-context).
    XZR = 32,
}

impl Gpr {
    /// 5-bit instruction-encoding index. SP and XZR both map to 31.
    pub const fn idx(self) -> u32 {
        match self {
            Gpr::SP | Gpr::XZR => 31,
            other => other as u32,
        }
    }
}

/// SIMD/FP 128-bit registers `v0..v31`. The same register slot is
/// addressable as `B/H/S/D/Q` (8/16/32/64/128-bit views); torajs uses
/// the 64-bit `D` view for `f64` arithmetic. The encoding index 0..31
/// is shared across views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum Fpr {
    V0 = 0,
    V1 = 1,
    V2 = 2,
    V3 = 3,
    V4 = 4,
    V5 = 5,
    V6 = 6,
    V7 = 7,
    V8 = 8,
    V9 = 9,
    V10 = 10,
    V11 = 11,
    V12 = 12,
    V13 = 13,
    V14 = 14,
    V15 = 15,
    V16 = 16,
    V17 = 17,
    V18 = 18,
    V19 = 19,
    V20 = 20,
    V21 = 21,
    V22 = 22,
    V23 = 23,
    V24 = 24,
    V25 = 25,
    V26 = 26,
    V27 = 27,
    V28 = 28,
    V29 = 29,
    V30 = 30,
    V31 = 31,
}

impl Fpr {
    pub const fn idx(self) -> u32 {
        self as u32
    }
}

/// Unified register handle. Allocator decides per-value whether the
/// value lives in a GPR or FPR based on SSA `Type` (int / ptr-shaped
/// → Gpr; F64 → Fpr).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    Gpr(Gpr),
    Fpr(Fpr),
}

impl Reg {
    /// Extract the GPR view. Panics if the slot holds an FPR — caller
    /// should know from instruction context which class is expected.
    pub fn as_gpr(self) -> Gpr {
        match self {
            Reg::Gpr(g) => g,
            Reg::Fpr(f) => panic!("expected Gpr, got Fpr({f:?})"),
        }
    }

    /// Extract the FPR view. Panics if the slot holds a GPR.
    pub fn as_fpr(self) -> Fpr {
        match self {
            Reg::Fpr(f) => f,
            Reg::Gpr(g) => panic!("expected Fpr, got Gpr({g:?})"),
        }
    }
}

/// AAPCS64 register class for trivial allocation.
///
/// Used by `regalloc` to pick scratch registers from the caller-saved
/// pool without colliding with the arg/return convention.
#[allow(dead_code)]
pub mod aapcs64 {
    use super::Gpr;

    /// Argument + return slot (also caller-saved). x0 holds the
    /// integer / pointer return value.
    pub const ARG_RET: [Gpr; 8] = [
        Gpr::X0,
        Gpr::X1,
        Gpr::X2,
        Gpr::X3,
        Gpr::X4,
        Gpr::X5,
        Gpr::X6,
        Gpr::X7,
    ];

    /// Caller-saved scratch (besides ARG_RET). 14 slots.
    pub const CALLER_SAVED_SCRATCH: [Gpr; 14] = [
        Gpr::X9,
        Gpr::X10,
        Gpr::X11,
        Gpr::X12,
        Gpr::X13,
        Gpr::X14,
        Gpr::X15,
        Gpr::X16,
        Gpr::X17,
        // x18 reserved on Apple platforms.
        Gpr::X19,
        Gpr::X20,
        Gpr::X21,
        Gpr::X22,
        Gpr::X23,
    ];

    use super::Fpr;

    /// FP argument + return slots (also caller-saved). `v0` (`d0`)
    /// holds the f64 return value.
    pub const FP_ARG_RET: [Fpr; 8] = [
        Fpr::V0,
        Fpr::V1,
        Fpr::V2,
        Fpr::V3,
        Fpr::V4,
        Fpr::V5,
        Fpr::V6,
        Fpr::V7,
    ];

    /// FP caller-saved scratch (besides `FP_ARG_RET`).
    /// `v8..v15` are callee-saved (low 64 bits only — AAPCS64 §5.1.2);
    /// `v16..v31` are caller-saved scratch. We restrict the scratch
    /// pool to the caller-saved 16-slot subset so codegen does not
    /// need to emit FP-save prologues yet.
    pub const FP_CALLER_SAVED_SCRATCH: [Fpr; 16] = [
        Fpr::V16,
        Fpr::V17,
        Fpr::V18,
        Fpr::V19,
        Fpr::V20,
        Fpr::V21,
        Fpr::V22,
        Fpr::V23,
        Fpr::V24,
        Fpr::V25,
        Fpr::V26,
        Fpr::V27,
        Fpr::V28,
        Fpr::V29,
        Fpr::V30,
        Fpr::V31,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idx_matches_repr() {
        assert_eq!(Gpr::X0.idx(), 0);
        assert_eq!(Gpr::X9.idx(), 9);
        assert_eq!(Gpr::X29.idx(), 29);
        assert_eq!(Gpr::X30.idx(), 30);
    }

    #[test]
    fn sp_and_xzr_both_encode_as_31() {
        assert_eq!(Gpr::SP.idx(), 31);
        assert_eq!(Gpr::XZR.idx(), 31);
    }

    #[test]
    fn aapcs64_arg_ret_covers_x0_x7() {
        assert_eq!(aapcs64::ARG_RET[0], Gpr::X0);
        assert_eq!(aapcs64::ARG_RET[7], Gpr::X7);
    }

    #[test]
    fn aapcs64_scratch_skips_x18_on_apple() {
        // Apple platforms reserve x18. The scratch pool must not
        // include it.
        assert!(!aapcs64::CALLER_SAVED_SCRATCH.contains(&Gpr::X18));
    }

    #[test]
    fn fpr_idx_matches_repr() {
        assert_eq!(Fpr::V0.idx(), 0);
        assert_eq!(Fpr::V8.idx(), 8);
        assert_eq!(Fpr::V31.idx(), 31);
    }

    #[test]
    fn aapcs64_fp_arg_ret_covers_v0_v7() {
        assert_eq!(aapcs64::FP_ARG_RET[0], Fpr::V0);
        assert_eq!(aapcs64::FP_ARG_RET[7], Fpr::V7);
    }

    #[test]
    fn aapcs64_fp_scratch_skips_callee_saved() {
        // v8..v15 are callee-saved (low 64 bits) per AAPCS64 §5.1.2.
        // Our scratch pool must not include them — we'd need a save/
        // restore in prologue/epilogue otherwise.
        for v in [
            Fpr::V8,
            Fpr::V9,
            Fpr::V10,
            Fpr::V11,
            Fpr::V12,
            Fpr::V13,
            Fpr::V14,
            Fpr::V15,
        ] {
            assert!(
                !aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&v),
                "{v:?} should not be in caller-saved scratch"
            );
        }
    }
}
