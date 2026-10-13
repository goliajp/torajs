//! 128-bit vector (Q-form) moves, loads and stores.

use crate::reg::{Fpr, Gpr};

/// LDR Qt, [Xn, #imm12] — load 128 bits, unsigned offset scaled by
/// 16. ARM ARM C7.2.191 (size=00, V=1, opc=11). Base = 0x3DC0_0000.
pub fn ldr_q_imm12(rt: Fpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset.is_multiple_of(16) && byte_offset / 16 < 4096);
    0x3DC0_0000 | ((byte_offset / 16) << 10) | (rn.idx() << 5) | rt.idx()
}

/// STR Qt, [Xn, #imm12] — store 128 bits, unsigned offset scaled by
/// 16. ARM ARM C7.2.330 (size=00, V=1, opc=10). Base = 0x3D80_0000.
pub fn str_q_imm12(rt: Fpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset.is_multiple_of(16) && byte_offset / 16 < 4096);
    0x3D80_0000 | ((byte_offset / 16) << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDUR Qt, [Xn, #simm9] — load 128 bits, unscaled signed offset.
/// ARM ARM C7.2.194. Base = 0x3CC0_0000.
pub fn ldur_q_imm9(rt: Fpr, rn: Gpr, byte_offset: i32) -> u32 {
    debug_assert!((-256..256).contains(&byte_offset));
    0x3CC0_0000 | (((byte_offset as u32) & 0x1FF) << 12) | (rn.idx() << 5) | rt.idx()
}

/// STUR Qt, [Xn, #simm9] — store 128 bits, unscaled signed offset.
/// ARM ARM C7.2.333. Base = 0x3C80_0000.
pub fn stur_q_imm9(rt: Fpr, rn: Gpr, byte_offset: i32) -> u32 {
    debug_assert!((-256..256).contains(&byte_offset));
    0x3C80_0000 | (((byte_offset as u32) & 0x1FF) << 12) | (rn.idx() << 5) | rt.idx()
}

/// ORR Vd.16B, Vn.16B, Vm.16B — bitwise or of the full register.
/// ARM ARM C7.2.215. Base = 0x4EA0_1C00.
pub fn orr_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4EA0_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// UMOV Xd, Vn.D[lane] — read one 64-bit lane into a GPR (lane 0 is
/// the `mov x, v.d[0]` alias). ARM ARM C7.2.371, imm5 = lane:1000.
/// Base = 0x4E00_3C00.
pub fn umov_x_d(rd: Gpr, rn: Fpr, lane: u32) -> u32 {
    debug_assert!(lane < 2);
    0x4E00_3C00 | (((lane << 4) | 0b1000) << 16) | (rn.idx() << 5) | rd.idx()
}

/// FDIV Vd.2D, Vn.2D, Vm.2D — lane-wise f64 divide. ARM ARM C7.2.106
/// (Q=1, sz=1). Base = 0x6E60_FC00.
pub fn fdiv_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E60_FC00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FADD Vd.2D, Vn.2D, Vm.2D — lane-wise f64 add. ARM ARM C7.2.49
/// (Q=1, sz=1). Base = 0x4E60_D400.
pub fn fadd_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4E60_D400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FCVTZS Vd.2D, Vn.2D — each f64 lane to i64, round toward zero,
/// saturating. ARM ARM C7.2.79 (Q=1, sz=1). Base = 0x4EE1_B800.
pub fn fcvtzs_v2d(rd: Fpr, rn: Fpr) -> u32 {
    0x4EE1_B800 | (rn.idx() << 5) | rd.idx()
}

/// FRINTZ Vd.2D, Vn.2D — each f64 lane rounded toward zero, kept as
/// f64. ARM ARM C7.2.150 (Q=1, sz=1). Base = 0x4EE1_9800.
pub fn frintz_v2d(rd: Fpr, rn: Fpr) -> u32 {
    0x4EE1_9800 | (rn.idx() << 5) | rd.idx()
}

/// FCMEQ Vd.2D, Vn.2D, Vm.2D — each lane all ones where the f64 lanes
/// compare equal (NaN never does), else zero. ARM ARM C7.2.58
/// (register, Q=1, sz=1). Base = 0x4E60_E400.
pub fn fcmeq_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4E60_E400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// AND Vd.16B, Vn.16B, Vm.16B — bitwise and of the full register.
/// ARM ARM C7.2.11. Base = 0x4E20_1C00.
pub fn and_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4E20_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// DUP Vd.2D, Vn.D[0] — broadcast the low D lane into both lanes.
/// ARM ARM C7.2.40 (imm5 = 01000). Base = 0x4E08_0400.
pub fn dup_2d_d(rd: Fpr, rn: Fpr) -> u32 {
    0x4E08_0400 | (rn.idx() << 5) | rd.idx()
}

/// INS Vd.D[1], Vn.D[0] — write the low D lane of Vn into the high D
/// lane of Vd. ARM ARM C7.2.175 (imm5 = 11000, imm4 = 0000).
/// Base = 0x6E18_0400.
pub fn ins_d1_d0(rd: Fpr, rn: Fpr) -> u32 {
    0x6E18_0400 | (rn.idx() << 5) | rd.idx()
}

/// FMUL / FSUB Vd.2D, Vn.2D, Vm.2D — lane-wise f64 multiply and
/// subtract. ARM ARM C7.2.136 / C7.2.157 (Q=1, sz=1). Bases
/// 0x6E60_DC00 and 0x4EE0_D400.
pub fn fmul_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E60_DC00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

pub fn fsub_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4EE0_D400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// SUB Vd.2D, Vn.2D, Vm.2D — lane-wise i64 subtract. ARM ARM
/// C7.2.341 (Q=1, size=11). Base = 0x6EE0_8400.
pub fn sub_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6EE0_8400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FCMGT / FCMGE Vd.2D, Vn.2D, Vm.2D — each lane all ones where the
/// f64 lanes compare `n > m` / `n >= m` (NaN never does), else zero.
/// ARM ARM C7.2.62 / C7.2.60 (register, Q=1, sz=1). Bases 0x6EE0_E400
/// and 0x6E60_E400.
pub fn fcmgt_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6EE0_E400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

pub fn fcmge_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E60_E400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// CMEQ / CMGT / CMGE Vd.2D, Vn.2D, Vm.2D — each lane all ones where
/// the signed i64 lanes compare `n == m` / `n > m` / `n >= m`, else
/// zero. ARM ARM C7.2.29 / C7.2.33 / C7.2.31 (register, Q=1,
/// size=11). Bases 0x6EE0_8C00, 0x4EE0_3400 and 0x4EE0_3C00.
pub fn cmeq_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6EE0_8C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

pub fn cmgt_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4EE0_3400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

pub fn cmge_v2d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4EE0_3C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// BIC Vd.16B, Vn.16B, Vm.16B — `n & !m` of the full register. ARM
/// ARM C7.2.20. Base = 0x4E60_1C00.
pub fn bic_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x4E60_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// BSL Vd.16B, Vn.16B, Vm.16B — bitwise select: Vd holds the mask on
/// entry and takes Vn's bits where it is set, Vm's where clear. ARM
/// ARM C7.2.23. Base = 0x6E60_1C00.
pub fn bsl_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E60_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// BIT Vd.16B, Vn.16B, Vm.16B — insert Vn's bits into Vd where the
/// mask Vm is set. ARM ARM C7.2.22. Base = 0x6EA0_1C00.
pub fn bit_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6EA0_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// BIF Vd.16B, Vn.16B, Vm.16B — insert Vn's bits into Vd where the
/// mask Vm is clear. ARM ARM C7.2.21. Base = 0x6EE0_1C00.
pub fn bif_v16b(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6EE0_1C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// EXT Vd.16B, Vn.16B, Vm.16B, #8 — the high D lane of Vn then the
/// low D lane of Vm. ARM ARM C7.2.45 (Q=1, imm4=8). Base =
/// 0x6E00_4000.
pub fn ext_v16b_8(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x6E00_4000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// MOVI Vd.2D, #0xffffffffffffffff — both lanes all ones. ARM ARM
/// C7.2.204 (64-bit variant, imm8 = 0xff). Base = 0x6F07_E7E0.
pub fn movi_2d_ones(rd: Fpr) -> u32 {
    0x6F07_E7E0 | rd.idx()
}

/// DUP Dd, Vn.D[1] (the `mov d, v.d[1]` alias) — the high D lane into
/// a scalar register, the rest of Vd cleared. ARM ARM C7.2.41 (scalar,
/// imm5 = 11000). Base = 0x5E18_0400.
pub fn dup_d_d1(rd: Fpr, rn: Fpr) -> u32 {
    0x5E18_0400 | (rn.idx() << 5) | rd.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    // golden words: clang -c -arch arm64 then otool -t
    //   ldr  q3, [x9, #48]      3dc00d23
    //   ldr  q0, [x1]           3dc00020
    //   str  q5, [x2, #32]      3d800845
    //   ldur q7, [x4, #-16]     3cdf0087
    //   stur q7, [x4, #-16]     3c9f0087
    //   orr  v1.16b, v2.16b, v3.16b  4ea31c41
    //   umov x5, v6.d[1]        4e183cc5
    //   mov  x5, v6.d[0]        4e083cc5
    //   fdiv v1.2d, v2.2d, v3.2d  6e63fc41
    //   fadd v1.2d, v2.2d, v3.2d  4e63d441
    //   fcvtzs v1.2d, v2.2d     4ee1b841
    //   dup  v1.2d, v2.d[0]     4e080441
    //   mov  v5.d[1], v6.d[0]   6e1804c5
    //   frintz v1.2d, v2.2d     4ee19841
    //   frintz v5.2d, v30.2d    4ee19bc5
    //   fcmeq v1.2d, v2.2d, v3.2d     4e63e441
    //   fcmeq v7.2d, v16.2d, v29.2d   4e7de607
    //   and  v1.16b, v2.16b, v3.16b   4e231c41
    //   and  v7.16b, v16.16b, v29.16b 4e3d1e07
    #[test]
    fn q_loads_and_stores_match_clang() {
        assert_eq!(ldr_q_imm12(Fpr::V3, Gpr::X9, 48), 0x3DC0_0D23);
        assert_eq!(ldr_q_imm12(Fpr::V0, Gpr::X1, 0), 0x3DC0_0020);
        assert_eq!(str_q_imm12(Fpr::V5, Gpr::X2, 32), 0x3D80_0845);
        assert_eq!(ldur_q_imm9(Fpr::V7, Gpr::X4, -16), 0x3CDF_0087);
        assert_eq!(stur_q_imm9(Fpr::V7, Gpr::X4, -16), 0x3C9F_0087);
    }

    #[test]
    fn orr_and_umov_match_clang() {
        assert_eq!(orr_v16b(Fpr::V1, Fpr::V2, Fpr::V3), 0x4EA3_1C41);
        assert_eq!(umov_x_d(Gpr::X5, Fpr::V6, 1), 0x4E18_3CC5);
        assert_eq!(umov_x_d(Gpr::X5, Fpr::V6, 0), 0x4E08_3CC5);
    }

    #[test]
    fn f64_lane_forms_match_clang() {
        assert_eq!(fdiv_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x6E63_FC41);
        assert_eq!(fadd_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x4E63_D441);
        assert_eq!(fcvtzs_v2d(Fpr::V1, Fpr::V2), 0x4EE1_B841);
        assert_eq!(dup_2d_d(Fpr::V1, Fpr::V2), 0x4E08_0441);
        assert_eq!(ins_d1_d0(Fpr::V5, Fpr::V6), 0x6E18_04C5);
    }

    #[test]
    fn divisibility_mask_forms_match_clang() {
        assert_eq!(frintz_v2d(Fpr::V1, Fpr::V2), 0x4EE1_9841);
        assert_eq!(frintz_v2d(Fpr::V5, Fpr::V30), 0x4EE1_9BC5);
        assert_eq!(fcmeq_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x4E63_E441);
        assert_eq!(fcmeq_v2d(Fpr::V7, Fpr::V16, Fpr::V29), 0x4E7D_E607);
        assert_eq!(and_v16b(Fpr::V1, Fpr::V2, Fpr::V3), 0x4E23_1C41);
        assert_eq!(and_v16b(Fpr::V7, Fpr::V16, Fpr::V29), 0x4E3D_1E07);
    }

    // clang golden words for the escape-time lane forms:
    //   fmul.2d v1, v2, v3      6e63dc41    fmul.2d v7, v16, v29    6e7dde07
    //   fsub.2d v1, v2, v3      4ee3d441    sub.2d v7, v16, v29     6efd8607
    //   fcmgt.2d v7, v16, v29   6efde607    fcmge.2d v1, v2, v3     6e63e441
    //   cmgt.2d v7, v16, v29    4efd3607    cmge.2d v1, v2, v3      4ee33c41
    //   cmeq.2d v1, v2, v3      6ee38c41    bic.16b v7, v16, v29    4e7d1e07
    //   bsl.16b v7, v16, v29    6e7d1e07    bit.16b v1, v2, v3      6ea31c41
    //   bif.16b v1, v2, v3      6ee31c41    ext.16b v7, v16, v29, #8  6e1d4207
    //   movi.2d v22, #0xffffffffffffffff  6f07e7f6    mov d7, v29.d[1]  5e1807a7
    #[test]
    fn escape_time_lane_forms_match_clang() {
        assert_eq!(fmul_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x6E63_DC41);
        assert_eq!(fmul_v2d(Fpr::V7, Fpr::V16, Fpr::V29), 0x6E7D_DE07);
        assert_eq!(fsub_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x4EE3_D441);
        assert_eq!(sub_v2d(Fpr::V7, Fpr::V16, Fpr::V29), 0x6EFD_8607);
        assert_eq!(fcmgt_v2d(Fpr::V7, Fpr::V16, Fpr::V29), 0x6EFD_E607);
        assert_eq!(fcmge_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x6E63_E441);
        assert_eq!(cmgt_v2d(Fpr::V7, Fpr::V16, Fpr::V29), 0x4EFD_3607);
        assert_eq!(cmge_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x4EE3_3C41);
        assert_eq!(cmeq_v2d(Fpr::V1, Fpr::V2, Fpr::V3), 0x6EE3_8C41);
        assert_eq!(bic_v16b(Fpr::V7, Fpr::V16, Fpr::V29), 0x4E7D_1E07);
        assert_eq!(bsl_v16b(Fpr::V7, Fpr::V16, Fpr::V29), 0x6E7D_1E07);
        assert_eq!(bit_v16b(Fpr::V1, Fpr::V2, Fpr::V3), 0x6EA3_1C41);
        assert_eq!(bif_v16b(Fpr::V1, Fpr::V2, Fpr::V3), 0x6EE3_1C41);
        assert_eq!(ext_v16b_8(Fpr::V7, Fpr::V16, Fpr::V29), 0x6E1D_4207);
        assert_eq!(movi_2d_ones(Fpr::V22), 0x6F07_E7F6);
        assert_eq!(dup_d_d1(Fpr::V7, Fpr::V29), 0x5E18_07A7);
    }
}
