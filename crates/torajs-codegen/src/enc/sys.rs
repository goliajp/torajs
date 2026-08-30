//! System register access — MRS / MSR (ARM ARM C5.2, C6.2).
//!
//! Only the FPCR pair is encoded here, and only because the program
//! entry needs to set FPCR.DN once before any user code runs (see
//! `torajs-cli::cmd_build_synthesize`). Nothing in the compiled body
//! of a program touches a system register.

use crate::reg::Gpr;

/// FPCR — Floating-point Control Register. `op0=3, op1=3, CRn=4,
/// CRm=4, op2=0` (ARM ARM D17.2.35), which is the `S3_3_C4_C4_0`
/// system-register encoding shared by the MRS and MSR words below.
const FPCR_SYSREG: u32 = 0x0003_4400;

/// MRS Xt, FPCR — read the FP control register into `rt`.
/// ARM ARM C6.2.219.
pub fn mrs_fpcr(rt: Gpr) -> u32 {
    0xD538_0000 | FPCR_SYSREG | rt.idx()
}

/// MSR FPCR, Xt — write `rt` into the FP control register.
/// ARM ARM C6.2.222.
pub fn msr_fpcr(rt: Gpr) -> u32 {
    0xD518_0000 | FPCR_SYSREG | rt.idx()
}

/// ORR Xd, Xn, #(1 << bit) — 64-bit logical-immediate OR of a single
/// bit. ARM ARM C6.2.243; the immediate rides the bitmask encoding
/// (`N`=1 for the 64-bit element size, `imms`=0 for a run of one set
/// bit, `immr` = the right-rotate that moves it into place).
pub fn orr_imm_bit(rd: Gpr, rn: Gpr, bit: u32) -> u32 {
    debug_assert!(bit < 64, "bit index must be in range for a 64-bit ORR");
    let immr = (64 - bit) % 64;
    0xB240_0000 | (immr << 16) | (rn.idx() << 5) | rd.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mrs_fpcr_x16_matches_clang() {
        assert_eq!(mrs_fpcr(Gpr::X16), 0xD53B_4410);
    }

    #[test]
    fn msr_fpcr_x16_matches_clang() {
        assert_eq!(msr_fpcr(Gpr::X16), 0xD51B_4410);
    }

    #[test]
    fn orr_imm_bit_25_x16_matches_clang() {
        // `orr x16, x16, #0x2000000` as assembled by cctools `as`.
        assert_eq!(orr_imm_bit(Gpr::X16, Gpr::X16, 25), 0xB267_0210);
    }

    #[test]
    fn orr_imm_bit_0_is_immr_zero() {
        // Bit 0 needs no rotate — `orr x0, x0, #1`.
        assert_eq!(orr_imm_bit(Gpr::X0, Gpr::X0, 0), 0xB240_0000);
    }
}
