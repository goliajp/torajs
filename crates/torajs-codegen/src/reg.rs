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
}
