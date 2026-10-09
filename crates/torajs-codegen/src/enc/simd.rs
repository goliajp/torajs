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
}
