//! Floating-point (D-form) — arithmetic, compare, GPR↔FPR move,
//! int↔fp convert.

use crate::reg::{Fpr, Gpr};

/// FADD Dd, Dn, Dm — f64 add. ARM ARM C6.2.96. Base = 0x1E60_2800.
pub fn fadd_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FSUB Dd, Dn, Dm. ARM ARM C6.2.144.
pub fn fsub_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_3800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FMUL Dd, Dn, Dm. ARM ARM C6.2.131.
pub fn fmul_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_0800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FDIV Dd, Dn, Dm. ARM ARM C6.2.103.
pub fn fdiv_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_1800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FMOV Dd, Xn — reinterpret the 64-bit value in Xn as an f64. No
/// conversion. ARM ARM C6.2.116. Base = 0x9E67_0000.
pub fn fmov_d_from_x(rd: Fpr, rn: Gpr) -> u32 {
    0x9E67_0000 | (rn.idx() << 5) | rd.idx()
}

/// FMOV Xd, Dn — reinterpret the 64-bit value in Dn as a u64.
/// ARM ARM C6.2.115. Base = 0x9E66_0000.
pub fn fmov_x_from_d(rd: Gpr, rn: Fpr) -> u32 {
    0x9E66_0000 | (rn.idx() << 5) | rd.idx()
}

/// FMOV Dd, Dn — register-to-register f64 move (same FPR class).
/// ARM ARM C6.2.118 (FMOV register, scalar). Base = 0x1E60_4000.
pub fn fmov_d_to_d(rd: Fpr, rn: Fpr) -> u32 {
    0x1E60_4000 | (rn.idx() << 5) | rd.idx()
}

/// FCMP Dn, Dm — set NZCV from `Dn vs Dm`. NaN sets the "unordered"
/// pattern (N=0,Z=0,C=1,V=1). ARM ARM C6.2.98. Base = 0x1E60_2000.
pub fn fcmp_d(rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2000 | (rm.idx() << 16) | (rn.idx() << 5)
}

/// CNT Vd.8B, Vn.8B — per-byte population count over the low 64 bits.
/// Advanced SIMD two-register misc, Q=0 size=00. ARM ARM C7.2.35.
/// Base = 0x0E20_5800.
pub fn cnt_v8b(rd: Fpr, rn: Fpr) -> u32 {
    0x0E20_5800 | (rn.idx() << 5) | rd.idx()
}

/// ADDV Bd, Vn.8B — horizontal add of the 8 byte lanes into Bd; the
/// scalar write zeroes the rest of Vd, so a following `FMOV Xd, Dn`
/// reads the clean sum. Advanced SIMD across lanes, Q=0 size=00.
/// ARM ARM C7.2.7. Base = 0x0E31_B800.
pub fn addv_b_v8b(rd: Fpr, rn: Fpr) -> u32 {
    0x0E31_B800 | (rn.idx() << 5) | rd.idx()
}

/// SCVTF Dd, Xn — signed int64 → f64. ARM ARM C6.2.353. Base = 0x9E62_0000.
pub fn scvtf_d_x(rd: Fpr, rn: Gpr) -> u32 {
    0x9E62_0000 | (rn.idx() << 5) | rd.idx()
}

/// FCVTZS Xd, Dn — f64 → signed int64, round toward zero.
/// ARM ARM C6.2.114. Base = 0x9E78_0000.
pub fn fcvtzs_x_d(rd: Gpr, rn: Fpr) -> u32 {
    0x9E78_0000 | (rn.idx() << 5) | rd.idx()
}

/// LDR Dt, [Xn, #imm12] — load 64-bit FP, unsigned offset, scaled
/// by 8. ARM ARM C7.2.181 (LDR SIMD&FP imm, unsigned offset, 64-bit
/// form). Base = 0xFD40_0000 (size=11, V=1, opc=01).
pub fn ldr_d_imm12(rt: Fpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xFD40_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// STR Dt, [Xn, #imm12] — store 64-bit FP, unsigned offset, scaled
/// by 8. ARM ARM C7.2.391 (STR SIMD&FP imm, unsigned offset, 64-bit
/// form). Base = 0xFD00_0000 (size=11, V=1, opc=00).
pub fn str_d_imm12(rt: Fpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xFD00_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDR Dt, [Xn, Xm, LSL #0] — load 64-bit FP, register-indexed, no
/// scale (Xm is a raw byte index). ARM ARM C7.2.183 (LDR SIMD&FP,
/// register, 64-bit form: size=11, V=1, opc=01, option=011 UXTX, S=0).
/// Base = 0xFC60_6800 — mirrors `ldr_x_reg` with V=1.
pub fn ldr_d_reg(rt: Fpr, rn: Gpr, rm: Gpr) -> u32 {
    0xFC60_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// STR Dt, [Xn, Xm, LSL #0] — store 64-bit FP, register-indexed.
/// ARM ARM C7.2.393 (STR SIMD&FP, register, 64-bit form: size=11,
/// V=1, opc=00, option=011 UXTX, S=0). Base = 0xFC20_6800 — mirrors
/// `str_x_reg` with V=1.
pub fn str_d_reg(rt: Fpr, rn: Gpr, rm: Gpr) -> u32 {
    0xFC20_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fadd_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fadd_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_2820);
    }

    #[test]
    fn fsub_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fsub_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_3820);
    }

    #[test]
    fn fmul_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fmul_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_0820);
    }

    #[test]
    fn fdiv_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fdiv_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_1820);
    }

    #[test]
    fn fmov_d0_from_x9_matches_arm_arm() {
        assert_eq!(fmov_d_from_x(Fpr::V0, Gpr::X9), 0x9E67_0120);
    }

    #[test]
    fn fmov_x0_from_d9_matches_arm_arm() {
        assert_eq!(fmov_x_from_d(Gpr::X0, Fpr::V9), 0x9E66_0120);
    }

    #[test]
    fn fmov_d0_to_d0_is_base_word() {
        // Self-move = base encoding; same bit pattern as clang -O0
        // emits for `double f(double a){return a;}` when not elided.
        assert_eq!(fmov_d_to_d(Fpr::V0, Fpr::V0), 0x1E60_4000);
    }

    #[test]
    fn fmov_d1_from_d2_matches_arm_arm() {
        // rn=2 → 2<<5=0x40, rd=1 → 1. Base | 0x40 | 1.
        assert_eq!(fmov_d_to_d(Fpr::V1, Fpr::V2), 0x1E60_4041);
    }

    #[test]
    fn fcmp_d16_d17_matches_arm_arm() {
        assert_eq!(fcmp_d(Fpr::V16, Fpr::V17), 0x1E71_2200);
    }

    #[test]
    fn cnt_v0_v0_matches_clang() {
        // clang -arch arm64: `cnt v0.8b, v0.8b` → 0x0E20_5800.
        assert_eq!(cnt_v8b(Fpr::V0, Fpr::V0), 0x0E20_5800);
    }

    #[test]
    fn cnt_v3_v7_matches_clang() {
        // clang: `cnt v3.8b, v7.8b` → 0x0E20_58E3.
        assert_eq!(cnt_v8b(Fpr::V3, Fpr::V7), 0x0E20_58E3);
    }

    #[test]
    fn addv_b0_v0_matches_clang() {
        // clang: `addv b0, v0.8b` → 0x0E31_B800.
        assert_eq!(addv_b_v8b(Fpr::V0, Fpr::V0), 0x0E31_B800);
    }

    #[test]
    fn addv_b5_v2_matches_clang() {
        // clang: `addv b5, v2.8b` → 0x0E31_B845.
        assert_eq!(addv_b_v8b(Fpr::V5, Fpr::V2), 0x0E31_B845);
    }

    #[test]
    fn scvtf_d0_from_x9_matches_arm_arm() {
        assert_eq!(scvtf_d_x(Fpr::V0, Gpr::X9), 0x9E62_0120);
    }

    #[test]
    fn fcvtzs_x0_from_d16_matches_arm_arm() {
        assert_eq!(fcvtzs_x_d(Gpr::X0, Fpr::V16), 0x9E78_0200);
    }

    #[test]
    fn ldr_d0_sp_offset_zero_matches_arm_arm() {
        // LDR d0, [sp]   →   imm12 = 0, rn = sp (31), rt = 0
        // Base 0xFD400000 | (31 << 5) = 0xFD4003E0
        assert_eq!(ldr_d_imm12(Fpr::V0, Gpr::SP, 0), 0xFD40_03E0);
    }

    #[test]
    fn ldr_d18_sp_offset_16_matches_arm_arm() {
        // LDR d18, [sp, #16]   →   imm12 = 2 (16 / 8), rn = 31, rt = 18
        // Base | (2 << 10) | (31 << 5) | 18 = 0xFD400000 | 0x800 | 0x3E0 | 0x12
        assert_eq!(ldr_d_imm12(Fpr::V18, Gpr::SP, 16), 0xFD40_0BF2);
    }

    #[test]
    fn str_d0_sp_offset_zero_matches_arm_arm() {
        // STR d0, [sp]   →   base 0xFD000000 | (31 << 5) = 0xFD0003E0
        assert_eq!(str_d_imm12(Fpr::V0, Gpr::SP, 0), 0xFD00_03E0);
    }

    #[test]
    fn str_d18_sp_offset_24_matches_arm_arm() {
        // STR d18, [sp, #24]   →   imm12 = 3, rn = 31, rt = 18
        assert_eq!(str_d_imm12(Fpr::V18, Gpr::SP, 24), 0xFD00_0FF2);
    }

    #[test]
    fn ldr_d_reg_d0_x12_x10_matches_arm_arm() {
        // LDR d0, [x12, x10]   →   GPR variant 0xF86A_6980 with V=1
        // Base 0xFC60_6800 | (10 << 16) | (12 << 5) | 0
        // = 0xFC60_6800 | 0x000A_0000 | 0x0000_0180
        // = 0xFC6A_6980
        assert_eq!(ldr_d_reg(Fpr::V0, Gpr::X12, Gpr::X10), 0xFC6A_6980);
    }

    #[test]
    fn str_d_reg_d9_x12_x11_matches_arm_arm() {
        // STR d9, [x12, x11]   →   GPR variant 0xF82B_6989 with V=1
        // Base 0xFC20_6800 | (11 << 16) | (12 << 5) | 9 = 0xFC2B_6989
        assert_eq!(str_d_reg(Fpr::V9, Gpr::X12, Gpr::X11), 0xFC2B_6989);
    }
}
