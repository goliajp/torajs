//! Control flow + condition codes — branches, returns, conditional
//! set, breakpoint, PC-relative page address.

use crate::reg::Gpr;

/// AArch64 4-bit condition codes (ARM ARM C1.2.4). Used by CSET,
/// B.cond, CSINC, CCMP, and friends. Naming follows the ARM ARM
/// mnemonics; signed/unsigned variant pairs share suffixes
/// (`HS`=`CS`, `LO`=`CC`).
#[allow(dead_code)]
pub mod cond {
    pub const EQ: u8 = 0b0000;
    pub const NE: u8 = 0b0001;
    pub const HS: u8 = 0b0010; // also "CS"
    pub const LO: u8 = 0b0011; // also "CC"
    pub const MI: u8 = 0b0100;
    pub const PL: u8 = 0b0101;
    pub const VS: u8 = 0b0110;
    pub const VC: u8 = 0b0111;
    pub const HI: u8 = 0b1000;
    pub const LS: u8 = 0b1001;
    pub const GE: u8 = 0b1010;
    pub const LT: u8 = 0b1011;
    pub const GT: u8 = 0b1100;
    pub const LE: u8 = 0b1101;
    pub const AL: u8 = 0b1110;
    pub const NV: u8 = 0b1111;
}

/// RET Xn — return through `Rn` (defaults to X30 = LR per AAPCS64).
/// ARM ARM C6.2.265.
pub fn ret(rn: Gpr) -> u32 {
    0xD65F_0000 | (rn.idx() << 5)
}

/// B label — unconditional branch with 26-bit signed displacement
/// (scaled by 4). ARM ARM C6.2.31.
pub fn b_imm26(byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm26 = (byte_offset / 4) as u32 & 0x03FF_FFFF;
    0x1400_0000 | imm26
}

/// BL label — branch-with-link (call). Same 26-bit form as B, stores
/// PC+4 to X30. ARM ARM C6.2.41.
pub fn bl_imm26(byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm26 = (byte_offset / 4) as u32 & 0x03FF_FFFF;
    0x9400_0000 | imm26
}

/// BLR Xn — indirect call through `Xn`. Stores PC+4 to X30.
/// ARM ARM C6.2.39. Base = 0xD63F_0000.
pub fn blr_reg(rn: Gpr) -> u32 {
    0xD63F_0000 | (rn.idx() << 5)
}

/// B.cond label — conditional branch with 19-bit signed displacement
/// (scaled by 4). ARM ARM C6.2.32. Base = 0x5400_0000.
pub fn b_cond_imm19(cond: u8, byte_offset: i32) -> u32 {
    debug_assert!(cond < 16, "cond must fit in 4 bits");
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm19 = ((byte_offset / 4) as u32) & 0x7_FFFF;
    0x5400_0000 | (imm19 << 5) | (cond as u32 & 0xF)
}

/// CBZ Xt, label — Compare-and-Branch on Zero (64-bit). ARM ARM C6.2.46.
pub fn cbz_x(rt: Gpr, byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm19 = ((byte_offset / 4) as u32) & 0x7_FFFF;
    0xB400_0000 | (imm19 << 5) | rt.idx()
}

/// CBNZ Xt, label — Compare-and-Branch on Non-Zero (64-bit).
/// ARM ARM C6.2.47.
pub fn cbnz_x(rt: Gpr, byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm19 = ((byte_offset / 4) as u32) & 0x7_FFFF;
    0xB500_0000 | (imm19 << 5) | rt.idx()
}

/// BRK #imm16 — breakpoint. Used for `Terminator::Unreachable` —
/// clang/LLVM convention is `brk #0xfd`. ARM ARM C6.2.42.
pub fn brk_imm16(imm: u16) -> u32 {
    0xD420_0000 | ((imm as u32) << 5)
}

/// CSET Xd, <cond> — set Xd to 1 if `<cond>` holds, else 0.
/// Alias for `CSINC Xd, XZR, XZR, invert(<cond>)`. ARM ARM C6.2.60.
///
/// Pass the *uninverted* cond — CSET inverts internally for CSINC.
pub fn cset_cond(rd: Gpr, cond: u8) -> u32 {
    debug_assert!(cond < 16, "cond must fit in 4 bits");
    let inverted = (cond ^ 1) as u32;
    0x9A9F_07E0 | (inverted << 12) | rd.idx()
}

/// CSEL Xd, Xn, Xm, <cond> — Xd = cond ? Xn : Xm. ARM ARM C6.2.53.
/// Branchless conditional move; the `InstKind::Select` lowering.
pub fn csel_cond(rd: Gpr, rn: Gpr, rm: Gpr, cond: u8) -> u32 {
    debug_assert!(cond < 16, "cond must fit in 4 bits");
    0x9A80_0000 | (rm.idx() << 16) | ((cond as u32) << 12) | (rn.idx() << 5) | rd.idx()
}

/// ADRP Xd, label — PC-relative 4 KiB-aligned page address.
/// ARM ARM C6.2.10. 21-bit signed immediate split into `immlo`
/// (bits 30-29) and `immhi` (bits 23-5). Callers pass 0 and pair
/// with a Page21 reloc.
pub fn adrp(rd: Gpr, immediate: i32) -> u32 {
    let imm21 = (immediate & 0x1F_FFFF) as u32;
    let immlo = imm21 & 0x3;
    let immhi = (imm21 >> 2) & 0x7_FFFF;
    0x9000_0000 | (immlo << 29) | (immhi << 5) | rd.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ret_x30_matches_arm_arm() {
        assert_eq!(ret(Gpr::X30), 0xD65F_03C0);
    }

    #[test]
    fn b_zero_offset_matches_arm_arm() {
        assert_eq!(b_imm26(0), 0x1400_0000);
    }

    #[test]
    fn b_positive_offset() {
        assert_eq!(b_imm26(4), 0x1400_0001);
    }

    #[test]
    fn b_negative_offset() {
        assert_eq!(b_imm26(-4), 0x17FF_FFFF);
    }

    #[test]
    fn bl_zero_offset_matches_arm_arm() {
        assert_eq!(bl_imm26(0), 0x9400_0000);
    }

    #[test]
    fn blr_x12_matches_arm_arm() {
        assert_eq!(blr_reg(Gpr::X12), 0xD63F_0180);
    }

    #[test]
    fn b_cond_eq_zero_offset_matches_arm_arm() {
        assert_eq!(b_cond_imm19(cond::EQ, 0), 0x5400_0000);
    }

    #[test]
    fn b_cond_ne_plus_8() {
        assert_eq!(b_cond_imm19(cond::NE, 8), 0x5400_0041);
    }

    #[test]
    fn cbz_x9_zero_offset_matches_arm_arm() {
        assert_eq!(cbz_x(Gpr::X9, 0), 0xB400_0009);
    }

    #[test]
    fn cbnz_x9_plus_8() {
        assert_eq!(cbnz_x(Gpr::X9, 8), 0xB500_0049);
    }

    #[test]
    fn brk_fd_matches_llvm_convention() {
        // BRK #0xFD — clang/LLVM emits this for `unreachable`.
        assert_eq!(brk_imm16(0xFD), 0xD420_1FA0);
    }

    #[test]
    fn cset_eq_matches_arm_arm() {
        // CSET x0, EQ = CSINC x0, XZR, XZR, NE → 0x9A9F_17E0
        assert_eq!(cset_cond(Gpr::X0, cond::EQ), 0x9A9F_17E0);
    }

    #[test]
    fn cset_lt_to_x9() {
        // CSET x9, LT = CSINC x9, XZR, XZR, GE → 0x9A9F_A7E9
        assert_eq!(cset_cond(Gpr::X9, cond::LT), 0x9A9F_A7E9);
    }

    // csel golden bytes cross-checked against clang -arch arm64
    // (objdump of `csel x3,x1,x2,ne` / `csel x0,x0,x1,eq` /
    // `csel x5,x4,x9,gt`).
    #[test]
    fn csel_ne_matches_clang() {
        assert_eq!(csel_cond(Gpr::X3, Gpr::X1, Gpr::X2, cond::NE), 0x9A82_1023);
    }

    #[test]
    fn csel_eq_same_dst_matches_clang() {
        assert_eq!(csel_cond(Gpr::X0, Gpr::X0, Gpr::X1, cond::EQ), 0x9A81_0000);
    }

    #[test]
    fn csel_gt_matches_clang() {
        assert_eq!(csel_cond(Gpr::X5, Gpr::X4, Gpr::X9, cond::GT), 0x9A89_C085);
    }

    #[test]
    fn adrp_x0_immediate_zero() {
        assert_eq!(adrp(Gpr::X0, 0), 0x9000_0000);
    }

    #[test]
    fn adrp_x12_immediate_zero() {
        assert_eq!(adrp(Gpr::X12, 0), 0x9000_000C);
    }

    #[test]
    fn adrp_immediate_split_low_high() {
        // immediate=1 → immlo=01 → bit 29 set
        assert_eq!(adrp(Gpr::X0, 1), 0xB000_0000);
        // immediate=4 → immhi=1 → bit 5 set
        assert_eq!(adrp(Gpr::X0, 4), 0x9000_0020);
    }
}
