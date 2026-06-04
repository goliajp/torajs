//! AArch64 instruction encoders (S1 subset).
//!
//! Each emitter returns a `u32` instruction word in *host* byte order;
//! the function-level driver in `compile.rs` is responsible for
//! writing little-endian bytes (aarch64 is little-endian) to the
//! output stream.
//!
//! Every encoder has an inline unit test asserting the emitted word
//! bit-pattern matches the ARM ARM (DDI 0487) spec; this is the
//! ground-truth source for #6 — if every encoder is spec-conformant,
//! the only remaining failure mode upstream is wrong instruction
//! selection, which `compile.rs` is responsible for and catches via
//! its own end-to-end byte-equal tests.
//!
//! S1 coverage: MOVZ, MOVK, ADD (shifted register + immediate),
//! SUB (shifted register + immediate), RET, B (unconditional branch),
//! BL (branch-with-link). That's enough for `1 + 2` end-to-end plus
//! room for the S2 arithmetic surface to drop in without reshuffling.

use crate::reg::Gpr;

/// MOVZ Xd, #imm16, LSL #(hw*16) — move 16-bit immediate into Rd
/// with zeroes elsewhere, optionally shifted to a hw-quadrant.
///
/// ARM ARM C6.2.187: `1 10 100101 hw imm16 Rd`
/// (sf=1, opc=10 for MOVZ, fixed 100101 opcode).
///
/// `hw` is the LSL quadrant: 0=bits 0-15, 1=bits 16-31, 2=bits 32-47,
/// 3=bits 48-63. To materialize a full 64-bit constant requires up to
/// 4 instructions (MOVZ + 3 MOVKs) but S1 only needs hw=0.
pub fn movz_imm(rd: Gpr, imm16: u16, hw: u8) -> u32 {
    debug_assert!(hw < 4, "hw must be 0..=3");
    0xD280_0000 | ((hw as u32) << 21) | ((imm16 as u32) << 5) | rd.idx()
}

/// MOVK Xd, #imm16, LSL #(hw*16) — keep other bits, overwrite the
/// hw-quadrant with `imm16`. Used to build full 64-bit constants
/// in concert with MOVZ.
///
/// ARM ARM C6.2.186: `1 11 100101 hw imm16 Rd` (opc=11 for MOVK).
pub fn movk_imm(rd: Gpr, imm16: u16, hw: u8) -> u32 {
    debug_assert!(hw < 4, "hw must be 0..=3");
    0xF280_0000 | ((hw as u32) << 21) | ((imm16 as u32) << 5) | rd.idx()
}

/// ADD Xd, Xn, Xm, LSL #0 — shifted-register form, LSL=0 (no shift).
///
/// ARM ARM C6.2.4: `1 0 0 01011 00 0 Rm imm6 Rn Rd` (shift=00=LSL,
/// imm6=0).
pub fn add_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x8B00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ADD Xd, Xn, #imm12 — immediate form, sh=0 (no LSL #12).
///
/// ARM ARM C6.2.4: `1 0 0 100010 sh imm12 Rn Rd` (sh=0 means imm12
/// is interpreted bit-aligned, no shift).
///
/// `imm12` must fit in 12 bits (0..=4095). Larger constants go through
/// MOVZ/MOVK + `add_reg`.
pub fn add_imm(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0x9100_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// SUB Xd, Xn, Xm, LSL #0 — shifted-register form.
///
/// ARM ARM C6.2.376: `1 1 0 01011 00 0 Rm imm6 Rn Rd` (sf=1, op=1
/// signals SUB instead of ADD).
pub fn sub_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xCB00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// SUB Xd, Xn, #imm12 — immediate form.
///
/// ARM ARM C6.2.376: `1 1 0 100010 sh imm12 Rn Rd`.
pub fn sub_imm(rd: Gpr, rn: Gpr, imm12: u16) -> u32 {
    debug_assert!(imm12 < 4096, "imm12 must fit in 12 bits");
    0xD100_0000 | ((imm12 as u32) << 10) | (rn.idx() << 5) | rd.idx()
}

/// RET Xn — unconditional return through `Rn` (defaults to X30 = LR
/// per AAPCS64).
///
/// ARM ARM C6.2.265: `1101 0110 0101 1111 0000 00 Rn 00000`.
pub fn ret(rn: Gpr) -> u32 {
    0xD65F_0000 | (rn.idx() << 5)
}

/// B label — unconditional branch with 26-bit signed PC-relative
/// immediate. Caller must supply the *byte* offset; encoder divides
/// by 4 to get the instruction-count immediate.
///
/// ARM ARM C6.2.31: `0 00101 imm26`. imm26 is sign-extended,
/// multiplied by 4, and added to PC.
pub fn b_imm26(byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm26 = (byte_offset / 4) as u32 & 0x03FF_FFFF;
    0x1400_0000 | imm26
}

/// BL label — branch-with-link (call). Same 26-bit signed encoding as
/// B, but stores PC+4 into X30 before branching. ARM ARM C6.2.41:
/// `1 00101 imm26`.
///
/// For inter-function calls the caller almost always emits with
/// `byte_offset = 0` and records a CallSite reloc — torajs-link (#8)
/// fills the real displacement at link time.
pub fn bl_imm26(byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 4 == 0, "branch offset must be 4-byte aligned");
    let imm26 = (byte_offset / 4) as u32 & 0x03FF_FFFF;
    0x9400_0000 | imm26
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movz_x0_one_matches_arm_arm() {
        // ARM ARM C6.2.187 worked example: MOVZ x0, #1
        //   sf=1 opc=10 100101 hw=00 imm16=0x0001 Rd=00000
        //   = 1101 0010 1000 0000 0000 0000 0010 0000 = 0xD2800020
        assert_eq!(movz_imm(Gpr::X0, 1, 0), 0xD280_0020);
    }

    #[test]
    fn movz_x9_one_matches_arm_arm() {
        // Same as above with Rd=01001 (x9)
        //   = 1101 0010 1000 0000 0000 0000 0010 1001 = 0xD2800029
        assert_eq!(movz_imm(Gpr::X9, 1, 0), 0xD280_0029);
    }

    #[test]
    fn movz_x10_two_matches_arm_arm() {
        // imm16=2, Rd=01010
        //   = 1101 0010 1000 0000 0000 0000 0100 1010 = 0xD280004A
        assert_eq!(movz_imm(Gpr::X10, 2, 0), 0xD280_004A);
    }

    #[test]
    fn movz_with_hw_shifts_into_quadrants() {
        // MOVZ x0, #0xABCD, LSL #16 — hw=01, imm16=0xABCD, Rd=00000
        //   sf=1, opc=10, opcode=100101, hw=01 (bit 21 set), imm16=0xABCD, Rd=0
        //   word = 0xD280_0000 | (1 << 21) | (0xABCD << 5) | 0
        //        = 0xD280_0000 | 0x0020_0000 | 0x0015_79A0
        //        = 0xD2B5_79A0
        //
        // Bit-by-bit verify (bits 23-16):
        //   opcode 100101 occupies bits 28-23 → bit 23 = 1.
        //   hw=01 → bit 22 = 0, bit 21 = 1.
        //   imm16 bit 15 (msb) = 1 (0xABCD's high bit) → lives at bit 20.
        //   nibble bits 23-20 = 1011 = B  (NOT A as a naive derivation hits).
        assert_eq!(movz_imm(Gpr::X0, 0xABCD, 1), 0xD2B5_79A0);
    }

    #[test]
    fn movk_x0_one_matches_arm_arm() {
        // MOVK x0, #1, LSL #0
        //   sf=1 opc=11 100101 hw=00 imm16=1 Rd=00000
        //   = 1111 0010 1000 0000 0000 0000 0010 0000 = 0xF2800020
        assert_eq!(movk_imm(Gpr::X0, 1, 0), 0xF280_0020);
    }

    #[test]
    fn add_reg_x0_x9_x10_matches_arm_arm() {
        // ADD x0, x9, x10 (LSL #0)
        //   sf=1 op=0 S=0 01011 shift=00 0 Rm=01010 imm6=0 Rn=01001 Rd=00000
        //   = 1000 1011 0000 1010 0000 0001 0010 0000 = 0x8B0A0120
        assert_eq!(add_reg(Gpr::X0, Gpr::X9, Gpr::X10), 0x8B0A_0120);
    }

    #[test]
    fn add_imm_x0_x0_two_matches_arm_arm() {
        // ADD x0, x0, #2
        //   sf=1 op=0 S=0 100010 sh=0 imm12=000000000010 Rn=00000 Rd=00000
        //   = 1001 0001 0000 0000 0000 1000 0000 0000 = 0x91000800
        assert_eq!(add_imm(Gpr::X0, Gpr::X0, 2), 0x9100_0800);
    }

    #[test]
    fn sub_reg_x0_x1_x2_matches_arm_arm() {
        // SUB x0, x1, x2 — sf=1 op=1 S=0 → main = 0xCB00_0000
        //   Rm=00010 Rn=00001 Rd=00000
        //   = 0xCB02_0020
        assert_eq!(sub_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xCB02_0020);
    }

    #[test]
    fn sub_imm_x0_x0_one_matches_arm_arm() {
        // SUB x0, x0, #1 — main = 0xD100_0000, imm12=1 at bits 10-21
        //   = 0xD100_0400
        assert_eq!(sub_imm(Gpr::X0, Gpr::X0, 1), 0xD100_0400);
    }

    #[test]
    fn ret_x30_matches_arm_arm() {
        // RET x30 (default)
        //   1101 0110 0101 1111 0000 0011 1100 0000 = 0xD65F03C0
        //   (Rn=11110 → bits 5-9 = 11110 = 0x3C0 once shifted)
        assert_eq!(ret(Gpr::X30), 0xD65F_03C0);
    }

    #[test]
    fn b_zero_offset_matches_arm_arm() {
        // B . (branch to self, offset=0) = 0x14000000
        assert_eq!(b_imm26(0), 0x1400_0000);
    }

    #[test]
    fn b_positive_offset() {
        // B +4 (branch to next inst) — imm26 = 1
        //   = 0x14000001
        assert_eq!(b_imm26(4), 0x1400_0001);
    }

    #[test]
    fn b_negative_offset() {
        // B -4 (branch backward 1 inst) — imm26 = -1 = 0x03FFFFFF after mask
        //   = 0x17FFFFFF
        assert_eq!(b_imm26(-4), 0x17FF_FFFF);
    }

    #[test]
    fn bl_zero_offset_matches_arm_arm() {
        // BL . = 0x94000000
        assert_eq!(bl_imm26(0), 0x9400_0000);
    }
}
