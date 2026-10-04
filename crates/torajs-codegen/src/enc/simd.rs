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
}
