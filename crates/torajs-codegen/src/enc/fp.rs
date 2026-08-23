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

/// FCSEL Dd, Dn, Dm, <cond> — Dd = cond ? Dn : Dm. ARM ARM C6.2.100.
/// The FPR twin of `csel_cond`; the f64 `InstKind::Select` lowering.
/// Base = 0x1E60_0C00 (ftype=01 double, bit21=1, bits 11-10 = 11).
pub fn fcsel_d(rd: Fpr, rn: Fpr, rm: Fpr, cond: u8) -> u32 {
    debug_assert!(cond < 16, "cond must fit in 4 bits");
    0x1E60_0C00 | (rm.idx() << 16) | ((cond as u32) << 12) | (rn.idx() << 5) | rd.idx()
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

/// CNT Vd.16B, Vn.16B — per-byte population count over the full
/// 128-bit register (Q=1 form of `cnt_v8b`). ARM ARM C7.2.35.
/// Base = 0x4E20_5800. clang golden: `cnt v16.16b, v17.16b` =
/// 0x4E20_5A30.
pub fn cnt_v16b(rd: Fpr, rn: Fpr) -> u32 {
    0x4E20_5800 | (rn.idx() << 5) | rd.idx()
}

/// UDOT Vd.4S, Vn.16B, Vm.16B — unsigned dot product: each of the 4
/// S lanes accumulates the dot of a 4-byte group of Vn with the
/// matching group of Vm (ARMv8.4 DotProd; all Apple M-series).
/// Against an all-ones weight vector this is a horizontal 4-byte-sum
/// into 4×u32 — the LLVM popcount-reduction idiom. ARM ARM C7.2.365.
/// Base = 0x6E80_9400. clang golden: `udot v22.4s, v5.16b, v21.16b`
/// = 0x6E95_94B6.
pub fn udot_4s(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E80_9400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// UADALP Vd.2D, Vn.4S — unsigned add-accumulate long pairwise:
/// each D lane += the sum of its two source S lanes. Widens the
/// 4×u32 dot-product partials into 2×u64 accumulators without a
/// carried dependency on the multiply chain. ARM ARM C7.2.361.
/// Base = 0x6EA0_6800. clang golden: `uadalp v0.2d, v22.4s` =
/// 0x6EA0_6AC0.
pub fn uadalp_2d(rd: Fpr, rn: Fpr) -> u32 {
    0x6EA0_6800 | (rn.idx() << 5) | rd.idx()
}

/// ADD Vd.2D, Vn.2D, Vm.2D — vector 64-bit lane add (vector-domain
/// IV step / accumulator merge). ARM ARM C7.2.4. Base = 0x4EE0_8400.
/// clang golden: `add v1.2d, v2.2d, v3.2d` = 0x4EE3_8441.
pub fn add_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4EE0_8400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ADDP Dd, Vn.2D — scalar pairwise add of the two D lanes into Dd
/// (final reduction step; the scalar write zeroes the rest of Vd so
/// a following `FMOV Xd, Dn` reads the clean sum). ARM ARM C7.2.6.
/// Base = 0x5EF1_B800. clang golden: `addp d0, v1.2d` = 0x5EF1_B820.
pub fn addp_d_v2d(rd: Fpr, rn: Fpr) -> u32 {
    0x5EF1_B800 | (rn.idx() << 5) | rd.idx()
}

/// DUP Vd.2D, Xn — broadcast a 64-bit GPR into both D lanes (splat;
/// IV seed / step vectors). ARM ARM C7.2.39. Base = 0x4E08_0C00.
/// clang golden: `dup v6.2d, x9` = 0x4E08_0D26.
pub fn dup_2d_x(rd: Fpr, rn: Gpr) -> u32 {
    0x4E08_0C00 | (rn.idx() << 5) | rd.idx()
}

/// MOVI Vd.2D, #0 — zero the full vector register (accumulator
/// init; rename-stage zero idiom on Apple cores). ARM ARM C7.2.204
/// (64-bit variant, imm8 = 0). Base = 0x6F00_E400. clang golden:
/// `movi v22.2d, #0` = 0x6F00_E416.
pub fn movi_2d_zero(rd: Fpr) -> u32 {
    0x6F00_E400 | rd.idx()
}

/// INS Vd.D[1], Xn — write a 64-bit GPR into the high D lane,
/// keeping lane 0 (IV lane pair `{i, i+1}` build: DUP then INS).
/// ARM ARM C7.2.175 (INS general, imm5 = 0b11000 for D[1]).
/// Base = 0x4E18_1C00. clang golden: `ins v4.d[1], x12` =
/// 0x4E18_1D84.
pub fn ins_d1_x(rd: Fpr, rn: Gpr) -> u32 {
    0x4E18_1C00 | (rn.idx() << 5) | rd.idx()
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

/// LDR Dt, [Xn, Xm, LSL #3] — FP 64-bit register-indexed load with
/// the scale folded into the AGU (S = 1). clang: `ldr d0, [x1, x2,
/// lsl #3]` = 0xFC62_7820.
pub fn ldr_d_reg_lsl3(rt: Fpr, rn: Gpr, rm: Gpr) -> u32 {
    0xFC60_7800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// STR Dt, [Xn, Xm, LSL #3] — FP 64-bit register-indexed scaled
/// store. clang: `str d0, [x1, x2, lsl #3]` = 0xFC22_7820.
pub fn str_d_reg_lsl3(rt: Fpr, rn: Gpr, rm: Gpr) -> u32 {
    0xFC20_7800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enc::ctrl::cond;

    #[test]
    fn fadd_d0_d1_d2_matches_arm_arm() {
        assert_eq!(fadd_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_2820);
    }

    // FCSEL golden bytes — clang -target aarch64-apple-darwin, five
    // points spanning cond, rd/rn/rm and the high register file:
    //   fcsel d0,  d1,  d2,  ne  →  1e621c20
    //   fcsel d3,  d4,  d9,  eq  →  1e690c83
    //   fcsel d5,  d0,  d1,  gt  →  1e61cc05
    //   fcsel d18, d16, d17, lt  →  1e71be12
    //   fcsel d31, d31, d31, al  →  1e7fefff
    #[test]
    fn fcsel_d0_d1_d2_ne_matches_clang() {
        assert_eq!(fcsel_d(Fpr::V0, Fpr::V1, Fpr::V2, cond::NE), 0x1E62_1C20);
    }

    #[test]
    fn fcsel_d3_d4_d9_eq_matches_clang() {
        assert_eq!(fcsel_d(Fpr::V3, Fpr::V4, Fpr::V9, cond::EQ), 0x1E69_0C83);
    }

    #[test]
    fn fcsel_d5_d0_d1_gt_matches_clang() {
        assert_eq!(fcsel_d(Fpr::V5, Fpr::V0, Fpr::V1, cond::GT), 0x1E61_CC05);
    }

    #[test]
    fn fcsel_d18_d16_d17_lt_matches_clang() {
        assert_eq!(fcsel_d(Fpr::V18, Fpr::V16, Fpr::V17, cond::LT), 0x1E71_BE12);
    }

    #[test]
    fn fcsel_d31_d31_d31_al_matches_clang() {
        assert_eq!(fcsel_d(Fpr::V31, Fpr::V31, Fpr::V31, cond::AL), 0x1E7F_EFFF);
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

    /// ctpop-range-sum canned-loop encoders vs clang golden bytes
    /// (`clang -c -march=armv8.4-a+dotprod`, assembled on mini
    /// 2026-07-19; RFC 20260719-ctpop-range-sum blade 1a).
    #[test]
    fn ctpop_reduction_simd_encoders_match_clang() {
        // cnt v16.16b, v17.16b
        assert_eq!(cnt_v16b(Fpr::V16, Fpr::V17), 0x4E20_5A30);
        // udot v22.4s, v5.16b, v21.16b
        assert_eq!(udot_4s(Fpr::V22, Fpr::V5, Fpr::V21), 0x6E95_94B6);
        // uadalp v0.2d, v22.4s
        assert_eq!(uadalp_2d(Fpr::V0, Fpr::V22), 0x6EA0_6AC0);
        // add v1.2d, v2.2d, v3.2d
        assert_eq!(add_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x4EE3_8441);
        // addp d0, v1.2d
        assert_eq!(addp_d_v2d(Fpr::V0, Fpr::V1), 0x5EF1_B820);
        // dup v6.2d, x9
        assert_eq!(dup_2d_x(Fpr::V6, Gpr::X9), 0x4E08_0D26);
        // movi v22.2d, #0
        assert_eq!(movi_2d_zero(Fpr::V22), 0x6F00_E416);
        // ins v4.d[1], x12 / ins v1.d[1], x9 (clang-assembled on
        // mini 2026-07-19; blade 1b IV lane-pair build)
        assert_eq!(ins_d1_x(Fpr::V4, Gpr::X12), 0x4E18_1D84);
        assert_eq!(ins_d1_x(Fpr::V1, Gpr::X9), 0x4E18_1D21);
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

    /// The four scaled (LSL #3) register-offset forms vs clang
    /// (`ldr/str x0/d0, [x1, x2, lsl #3]` assembled + objdump'd).
    #[test]
    fn scaled_reg_forms_match_clang() {
        use crate::enc::mem::{ldr_x_reg_lsl3, str_x_reg_lsl3};
        assert_eq!(ldr_x_reg_lsl3(Gpr::X0, Gpr::X1, Gpr::X2), 0xF862_7820);
        assert_eq!(str_x_reg_lsl3(Gpr::X0, Gpr::X1, Gpr::X2), 0xF822_7820);
        assert_eq!(ldr_d_reg_lsl3(Fpr::V0, Gpr::X1, Gpr::X2), 0xFC62_7820);
        assert_eq!(str_d_reg_lsl3(Fpr::V0, Gpr::X1, Gpr::X2), 0xFC22_7820);
    }

    #[test]
    fn str_d_reg_d9_x12_x11_matches_arm_arm() {
        // STR d9, [x12, x11]   →   GPR variant 0xF82B_6989 with V=1
        // Base 0xFC20_6800 | (11 << 16) | (12 << 5) | 9 = 0xFC2B_6989
        assert_eq!(str_d_reg(Fpr::V9, Gpr::X12, Gpr::X11), 0xFC2B_6989);
    }
}
