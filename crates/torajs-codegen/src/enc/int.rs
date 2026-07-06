//! Integer data-processing — moves, arithmetic, logic, shifts, compare.

use crate::reg::Gpr;

/// MOVZ Xd, #imm16, LSL #(hw*16) — move 16-bit immediate into Rd with
/// zeroes elsewhere. ARM ARM C6.2.187. `hw` is the LSL quadrant
/// (0..=3). Building a 64-bit constant needs up to 4 instructions
/// (MOVZ + 3 MOVKs).
pub fn movz_imm(rd: Gpr, imm16: u16, hw: u8) -> u32 {
    debug_assert!(hw < 4, "hw must be 0..=3");
    0xD280_0000 | ((hw as u32) << 21) | ((imm16 as u32) << 5) | rd.idx()
}

/// MOVK Xd, #imm16, LSL #(hw*16) — keep other bits, overwrite the
/// hw-quadrant. ARM ARM C6.2.186.
pub fn movk_imm(rd: Gpr, imm16: u16, hw: u8) -> u32 {
    debug_assert!(hw < 4, "hw must be 0..=3");
    0xF280_0000 | ((hw as u32) << 21) | ((imm16 as u32) << 5) | rd.idx()
}

/// ADD Xd, Xn, Xm, LSL #0. ARM ARM C6.2.4.
pub fn add_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x8B00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ADD Xd, Xn, #imm12. ARM ARM C6.2.4. `imm12 < 4096`.
pub fn add_imm(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0x9100_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// ADD Xd, Xn, #imm12, LSL #12 — shifted-immediate form (sh=1),
/// adds `imm12 * 4096`. Pairs with the plain form for frame sizes
/// past imm12. ARM ARM C6.2.4.
pub fn add_imm_lsl12(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0x9140_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// SUB Xd, Xn, Xm, LSL #0. ARM ARM C6.2.376.
pub fn sub_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xCB00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// SUB Xd, Xn, #imm12. ARM ARM C6.2.376.
pub fn sub_imm(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0xD100_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// SUB Xd, Xn, #imm12, LSL #12 — shifted-immediate form (sh=1),
/// subtracts `imm12 * 4096`. ARM ARM C6.2.376.
pub fn sub_imm_lsl12(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0xD140_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// MUL Xd, Xn, Xm — alias for MADD Xd, Xn, Xm, XZR. ARM ARM C6.2.222.
pub fn mul_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9B00_7C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// SDIV Xd, Xn, Xm — signed divide. ARM ARM C6.2.371. Trapless;
/// INT64_MIN / -1 → INT64_MIN; divide-by-zero → 0 (higher layers guard).
pub fn sdiv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_0C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// UDIV Xd, Xn, Xm — unsigned divide. ARM ARM C6.2.432.
pub fn udiv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_0800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// MSUB Xd, Xn, Xm, Ra — Ra - (Xn * Xm). Used to compute SREM:
/// `rem = a - (a / b) * b` via SDIV then MSUB. ARM ARM C6.2.220.
pub fn msub_reg(rd: Gpr, rn: Gpr, rm: Gpr, ra: Gpr) -> u32 {
    0x9B00_8000 | (rm.idx() << 16) | (ra.idx() << 10) | (rn.idx() << 5) | rd.idx()
}

/// AND Xd, Xn, Xm — logical AND, shifted-register form. ARM ARM C6.2.13.
pub fn and_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x8A00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ORR Xd, Xn, Xm — logical OR. ARM ARM C6.2.232.
pub fn orr_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xAA00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// EOR Xd, Xn, Xm — logical XOR. ARM ARM C6.2.91.
pub fn eor_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xCA00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// LSLV Xd, Xn, Xm — variable LSL (shift count mod 64). ARM ARM C6.2.157.
pub fn lslv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ASRV Xd, Xn, Xm — variable arithmetic shift right. ARM ARM C6.2.20.
pub fn asrv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// LSRV Xd, Xn, Xm — variable logical shift right. ARM ARM C6.2.161.
pub fn lsrv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// CMP Xn, Xm — alias for SUBS XZR, Xn, Xm. Sets NZCV based on
/// `Xn - Xm`; result discarded. ARM ARM C6.2.49 / C6.2.377.
pub fn cmp_reg(rn: Gpr, rm: Gpr) -> u32 {
    0xEB00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | 31
}

/// CMP Xn, #imm12 — alias for SUBS XZR, Xn, #imm12. Sets NZCV based
/// on `Xn - imm`; result discarded. ARM ARM C6.2.48 / C6.2.375.
/// Round 5 popcount branch/const attack — lets `icmp x, #small-const`
/// skip the `MOVZ scratch, #c` rematerialization per compare.
pub fn cmp_imm(rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0xF100_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | 31
}

/// CMP Wn, Wm — 32-bit form, alias for SUBS WZR, Wn, Wm. Sets NZCV
/// based on `Wn - Wm` (low 32 bits only); high 32 ignored. Needed
/// for I32-typed ICmp where the source 64-bit register slot has
/// non-zero high bits (e.g. `Load(I32, hdr, 0)` 64-bit-loads a heap
/// header whose high 32 bits hold `type_tag + flags` — a 64-bit
/// cmp would inadvertently compare those into the verdict and
/// misroute the rc_dec walk vs. cycle-buffer branch).
/// sf=0 → 0x6B00_0000 base (vs. 64-bit 0xEB00_0000).
pub fn cmp_w_reg(rn: Gpr, rm: Gpr) -> u32 {
    0x6B00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | 31
}

/// AND Xd, Xn, #1 — extract the low bit (ZExtBoolToI64 + Trunc-to-Bool).
/// ARM ARM C6.2.12 with bitmask encoding N=1, immr=0, imms=0.
pub fn and_imm_one(rd: Gpr, rn: Gpr) -> u32 {
    0x9240_0000 | (rn.idx() << 5) | rd.idx()
}

/// MOV Wd, Wn — 32-bit move via `ORR Wd, WZR, Wn`. The W-form write
/// auto-clears the top 32 bits of Xd → textbook ZExtI32ToI64.
/// ARM ARM C6.2.231 (alias) / C6.2.232 (sf=0 ORR).
pub fn mov_w_reg(rd: Gpr, rm: Gpr) -> u32 {
    0x2A00_03E0 | (rm.idx() << 16) | rd.idx()
}

/// MOV Xd, Xn — 64-bit move via `ORR Xd, XZR, Xn`. ARM ARM C6.2.231
/// (sf=1 form). Used for PtrToInt / IntToPtr.
pub fn mov_x_reg(rd: Gpr, rm: Gpr) -> u32 {
    0xAA00_03E0 | (rm.idx() << 16) | rd.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movz_x0_one_matches_arm_arm() {
        assert_eq!(movz_imm(Gpr::X0, 1, 0), 0xD280_0020);
    }

    #[test]
    fn movz_x9_one_matches_arm_arm() {
        assert_eq!(movz_imm(Gpr::X9, 1, 0), 0xD280_0029);
    }

    #[test]
    fn movz_x10_two_matches_arm_arm() {
        assert_eq!(movz_imm(Gpr::X10, 2, 0), 0xD280_004A);
    }

    #[test]
    fn movz_with_hw_shifts_into_quadrants() {
        // MOVZ x0, #0xABCD, LSL #16 — hw=01 (bit 21), imm16=0xABCD, Rd=0
        // Word: bits 23-20 = 1011 = B, not A — naive derivation hits A.
        assert_eq!(movz_imm(Gpr::X0, 0xABCD, 1), 0xD2B5_79A0);
    }

    #[test]
    fn movk_x0_one_matches_arm_arm() {
        assert_eq!(movk_imm(Gpr::X0, 1, 0), 0xF280_0020);
    }

    #[test]
    fn add_reg_x0_x9_x10_matches_arm_arm() {
        assert_eq!(add_reg(Gpr::X0, Gpr::X9, Gpr::X10), 0x8B0A_0120);
    }

    #[test]
    fn add_imm_x0_x0_two_matches_arm_arm() {
        assert_eq!(add_imm(Gpr::X0, Gpr::X0, 2), 0x9100_0800);
    }

    #[test]
    fn sub_reg_x0_x1_x2_matches_arm_arm() {
        assert_eq!(sub_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xCB02_0020);
    }

    #[test]
    fn sub_imm_x0_x0_one_matches_arm_arm() {
        assert_eq!(sub_imm(Gpr::X0, Gpr::X0, 1), 0xD100_0400);
    }

    #[test]
    fn mul_x0_x1_x2_matches_arm_arm() {
        assert_eq!(mul_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9B02_7C20);
    }

    #[test]
    fn sdiv_x0_x1_x2_matches_arm_arm() {
        assert_eq!(sdiv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_0C20);
    }

    #[test]
    fn udiv_x0_x1_x2_matches_arm_arm() {
        assert_eq!(udiv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_0820);
    }

    #[test]
    fn msub_x0_x9_x10_x1_matches_arm_arm() {
        assert_eq!(msub_reg(Gpr::X0, Gpr::X9, Gpr::X10, Gpr::X1), 0x9B0A_8520);
    }

    #[test]
    fn and_x0_x1_x2_matches_arm_arm() {
        assert_eq!(and_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x8A02_0020);
    }

    #[test]
    fn orr_x0_x1_x2_matches_arm_arm() {
        assert_eq!(orr_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xAA02_0020);
    }

    #[test]
    fn eor_x0_x1_x2_matches_arm_arm() {
        assert_eq!(eor_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xCA02_0020);
    }

    #[test]
    fn lslv_x0_x1_x2_matches_arm_arm() {
        assert_eq!(lslv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2020);
    }

    #[test]
    fn asrv_x0_x1_x2_matches_arm_arm() {
        assert_eq!(asrv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2820);
    }

    #[test]
    fn lsrv_x0_x1_x2_matches_arm_arm() {
        assert_eq!(lsrv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2420);
    }

    #[test]
    fn cmp_x9_imm5_matches_arm_arm() {
        // CMP X9, #5 = SUBS XZR, X9, #5
        assert_eq!(cmp_imm(Gpr::X9, 5), 0xF100_153F);
    }

    #[test]
    fn cmp_x9_x10_matches_arm_arm() {
        assert_eq!(cmp_reg(Gpr::X9, Gpr::X10), 0xEB0A_013F);
    }

    #[test]
    fn cmp_w9_w10_matches_arm_arm() {
        // CMP Wn, Wm = SUBS WZR, Wn, Wm. sf=0 form differs only in the
        // top sf bit cleared (0x6B vs 0xEB). Same Rn/Rm bit layout.
        assert_eq!(cmp_w_reg(Gpr::X9, Gpr::X10), 0x6B0A_013F);
    }

    #[test]
    fn and_imm_one_x0_x9_matches_clang() {
        assert_eq!(and_imm_one(Gpr::X0, Gpr::X9), 0x9240_0120);
    }

    #[test]
    fn mov_w_reg_w0_from_w9_matches_arm_arm() {
        assert_eq!(mov_w_reg(Gpr::X0, Gpr::X9), 0x2A09_03E0);
    }

    #[test]
    fn mov_x_reg_x0_from_x9_matches_arm_arm() {
        assert_eq!(mov_x_reg(Gpr::X0, Gpr::X9), 0xAA09_03E0);
    }
}
