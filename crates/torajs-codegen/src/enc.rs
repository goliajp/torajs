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
//!
//! S2-A coverage: MUL (3-op MADD with Ra=XZR), SDIV, UDIV, MSUB
//! (for SREM = a - (a/b)*b), AND/ORR/EOR (logical shifted register),
//! LSLV/ASRV/LSRV (variable shifts). Each new encoder has an inline
//! unit test vs ARM ARM bit patterns.
//!
//! S2-B1 coverage: f64 FADD/FSUB/FMUL/FDIV (FP data-processing
//! 2-source, type=01 D-form) + FMOV both directions between Xn and
//! Dn (FP move-general). FRem has no aarch64 instruction — it lowers
//! to a `fmod` libm call which S2-B2 routes via Call + the existing
//! `bl_imm26` reloc machinery.
//!
//! S2-C coverage: SUBS (CMP alias with Rd=XZR) for integer compare,
//! FCMP Dn,Dm for f64 compare, CSET Wd/Xd,<cond> (alias for CSINC
//! Wd,WZR,WZR,~<cond>) for converting NZCV → 0/1 boolean. The four-
//! bit aarch64 condition codes are exposed as constants so the
//! IPred/FPred → cond mapping in compile.rs reads as a direct table.

use crate::reg::{Fpr, Gpr};

/// AArch64 4-bit condition codes (ARM ARM C1.2.4).
///
/// Used by CSET, B.cond, CSINC, CCMP, and friends. Naming follows the
/// ARM ARM mnemonics; signed/unsigned variant pairs share suffixes
/// (e.g. `HS`=`CS`, `LO`=`CC`). For FCMP-driven CSET, see
/// `compile::fpred_to_cond` for the spec-conformant FP cond mapping.
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

// --- S2-A: integer multiply / divide / remainder ---

/// MUL Xd, Xn, Xm — alias for MADD Xd, Xn, Xm, XZR.
///
/// ARM ARM C6.2.222 (MUL alias) / C6.2.176 (MADD):
/// `1 00 11011 000 Rm 0 Ra Rn Rd` with Ra=11111 (XZR).
pub fn mul_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9B00_7C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// SDIV Xd, Xn, Xm — signed integer divide.
///
/// ARM ARM C6.2.371: `1 0 0 11010 110 Rm 000011 Rn Rd`.
/// Trapless: dividing INT64_MIN by -1 returns INT64_MIN; divide-by-0
/// returns 0 (aarch64 does NOT raise; higher layers must guard).
pub fn sdiv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_0C00 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// UDIV Xd, Xn, Xm — unsigned integer divide.
///
/// ARM ARM C6.2.432: `1 0 0 11010 110 Rm 000010 Rn Rd`.
pub fn udiv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_0800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// MSUB Xd, Xn, Xm, Ra — Ra - (Xn * Xm). Used to compute SREM:
/// `rem = a - (a / b) * b` (via SDIV then MSUB).
///
/// ARM ARM C6.2.220: `1 00 11011 000 Rm 1 Ra Rn Rd`.
pub fn msub_reg(rd: Gpr, rn: Gpr, rm: Gpr, ra: Gpr) -> u32 {
    0x9B00_8000 | (rm.idx() << 16) | (ra.idx() << 10) | (rn.idx() << 5) | rd.idx()
}

// --- S2-A: bitwise logic (shifted register form, no shift) ---

/// AND Xd, Xn, Xm — logical AND, shifted-register form, LSL #0.
///
/// ARM ARM C6.2.13: `1 00 01010 shift N Rm imm6 Rn Rd`
/// (sf=1, opc=00=AND, shift=00=LSL, N=0).
pub fn and_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x8A00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ORR Xd, Xn, Xm — logical OR.
///
/// ARM ARM C6.2.232: opc=01=ORR.
pub fn orr_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xAA00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// EOR Xd, Xn, Xm — logical XOR.
///
/// ARM ARM C6.2.91: opc=10=EOR.
pub fn eor_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xCA00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

// --- S2-A: variable shift (shift count in a register) ---

/// LSLV Xd, Xn, Xm — logical shift left by `Xm` (mod 64).
///
/// ARM ARM C6.2.157: `1 0 0 11010 110 Rm 001000 Rn Rd`. The shift
/// count is read as `Xm & 0x3F` (low 6 bits) — matches LLVM's
/// `shl i64` semantics for in-bound shifts; out-of-range UB upstream
/// is the lowerer's responsibility.
pub fn lslv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2000 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// ASRV Xd, Xn, Xm — arithmetic (sign-extending) shift right by Xm.
///
/// ARM ARM C6.2.20: opcode2=001010.
pub fn asrv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// LSRV Xd, Xn, Xm — logical (zero-fill) shift right by Xm.
///
/// ARM ARM C6.2.161: opcode2=001001.
pub fn lsrv_reg(rd: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x9AC0_2400 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

// --- S2-B1: f64 arithmetic (FP data-processing 2-source, type=01) ---

/// FADD Dd, Dn, Dm — f64 add.
///
/// ARM ARM C6.2.96: `0 0 0 11110 type 1 Rm opcode 10 Rn Rd`
/// type=01 (D), opcode=0010 (FADD).
pub fn fadd_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FSUB Dd, Dn, Dm — f64 subtract.
///
/// ARM ARM C6.2.144: opcode=0011.
pub fn fsub_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_3800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FMUL Dd, Dn, Dm — f64 multiply.
///
/// ARM ARM C6.2.131: opcode=0000.
pub fn fmul_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_0800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

/// FDIV Dd, Dn, Dm — f64 divide.
///
/// ARM ARM C6.2.103: opcode=0001.
pub fn fdiv_d(rd: Fpr, rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_1800 | (rm.idx() << 16) | (rn.idx() << 5) | rd.idx()
}

// --- S2-B1: GPR ↔ FPR moves (FP move, general) ---

/// FMOV Dd, Xn — move the 64-bit value in Xn to Dd (interpret the
/// bits as an f64; no conversion).
///
/// ARM ARM C6.2.116: `1 0 0 11110 01 1 00 111 000000 Rn Rd`
/// (sf=1, type=01 D, rmode=00, opcode=111).
pub fn fmov_d_from_x(rd: Fpr, rn: Gpr) -> u32 {
    0x9E67_0000 | (rn.idx() << 5) | rd.idx()
}

/// FMOV Xd, Dn — move the 64-bit value in Dn to Xd (interpret the
/// bits as a u64; no conversion).
///
/// ARM ARM C6.2.115: `1 0 0 11110 01 1 00 110 000000 Rn Rd`
/// (sf=1, type=01 D, rmode=00, opcode=110).
pub fn fmov_x_from_d(rd: Gpr, rn: Fpr) -> u32 {
    0x9E66_0000 | (rn.idx() << 5) | rd.idx()
}

// --- S2-C: compare + condition-set ---

/// CMP Xn, Xm — alias for SUBS XZR, Xn, Xm. Sets NZCV based on
/// `Xn - Xm`; result discarded.
///
/// ARM ARM C6.2.49 (CMP alias) / C6.2.377 (SUBS shifted register):
/// `1 1 1 01011 00 0 Rm imm6 Rn Rd` (Rd=XZR=31, sf=1, op=1, S=1).
pub fn cmp_reg(rn: Gpr, rm: Gpr) -> u32 {
    0xEB00_0000 | (rm.idx() << 16) | (rn.idx() << 5) | 31
}

/// FCMP Dn, Dm — set NZCV from the f64 comparison `Dn vs Dm`. Result
/// discarded. NaN handling: if either operand is NaN, the
/// "unordered" pattern N=0,Z=0,C=1,V=1 is set — useful for separating
/// One (ordered ≠) from Une (unordered or ≠).
///
/// ARM ARM C6.2.98: `0 0 0 11110 type 1 Rm 00 1000 Rn opcode2`
/// type=01 (D), opcode2=00000.
pub fn fcmp_d(rn: Fpr, rm: Fpr) -> u32 {
    0x1E60_2000 | (rm.idx() << 16) | (rn.idx() << 5)
}

/// CSET Xd, <cond> — set Xd to 1 if `<cond>` holds in NZCV, else 0.
/// Alias for `CSINC Xd, XZR, XZR, invert(<cond>)`.
///
/// ARM ARM C6.2.60 (CSET alias) / C6.2.62 (CSINC):
/// CSINC: `1 0 0 11010100 Rm cond 0 1 Rn Rd`
/// With Rm=XZR(31), Rn=XZR(31), cond_field = cond XOR 1 (low bit
/// toggled — CSET inverts the cond it's given to satisfy the CSINC
/// "increment-on-not-cond" semantics).
///
/// `cond` argument: the *uninverted* condition you want CSET to test
/// — e.g. pass `cond::EQ` to materialize 1 when equal.
pub fn cset_cond(rd: Gpr, cond: u8) -> u32 {
    debug_assert!(cond < 16, "cond must fit in 4 bits");
    let inverted = (cond ^ 1) as u32;
    0x9A9F_07E0 | (inverted << 12) | rd.idx()
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

    // --- S2-A encoder tests ---

    #[test]
    fn mul_x0_x1_x2_matches_arm_arm() {
        // MUL x0, x1, x2 = MADD x0, x1, x2, XZR
        //   base = 0x9B00_7C00 (Ra=XZR=31 baked in via 0x7C00 = 11111 at bits 14-10)
        //   Rm=2 (bits 20-16), Rn=1 (bits 9-5), Rd=0
        //   = 0x9B02_7C20
        assert_eq!(mul_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9B02_7C20);
    }

    #[test]
    fn sdiv_x0_x1_x2_matches_arm_arm() {
        // SDIV x0, x1, x2: base 0x9AC0_0C00, Rm=2, Rn=1, Rd=0
        //   = 0x9AC2_0C20
        assert_eq!(sdiv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_0C20);
    }

    #[test]
    fn udiv_x0_x1_x2_matches_arm_arm() {
        // UDIV x0, x1, x2: base 0x9AC0_0800
        //   = 0x9AC2_0820
        assert_eq!(udiv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_0820);
    }

    #[test]
    fn msub_x0_x9_x10_x1_matches_arm_arm() {
        // MSUB x0, x9, x10, x1: base 0x9B00_8000
        //   Rm=10 (bits 20-16), Ra=1 (bits 14-10), Rn=9 (bits 9-5), Rd=0
        //   = 0x9B00_8000 | (10<<16) | (1<<10) | (9<<5) | 0
        //   = 0x9B0A_8520
        assert_eq!(msub_reg(Gpr::X0, Gpr::X9, Gpr::X10, Gpr::X1), 0x9B0A_8520);
    }

    #[test]
    fn and_x0_x1_x2_matches_arm_arm() {
        // AND x0, x1, x2: base 0x8A00_0000, Rm=2, Rn=1, Rd=0
        //   = 0x8A02_0020
        assert_eq!(and_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x8A02_0020);
    }

    #[test]
    fn orr_x0_x1_x2_matches_arm_arm() {
        // ORR x0, x1, x2: base 0xAA00_0000
        //   = 0xAA02_0020
        assert_eq!(orr_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xAA02_0020);
    }

    #[test]
    fn eor_x0_x1_x2_matches_arm_arm() {
        // EOR x0, x1, x2: base 0xCA00_0000
        //   = 0xCA02_0020
        assert_eq!(eor_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xCA02_0020);
    }

    #[test]
    fn lslv_x0_x1_x2_matches_arm_arm() {
        // LSLV x0, x1, x2: base 0x9AC0_2000
        //   = 0x9AC2_2020
        assert_eq!(lslv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2020);
    }

    #[test]
    fn asrv_x0_x1_x2_matches_arm_arm() {
        // ASRV x0, x1, x2: base 0x9AC0_2800
        //   = 0x9AC2_2820
        assert_eq!(asrv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2820);
    }

    #[test]
    fn lsrv_x0_x1_x2_matches_arm_arm() {
        // LSRV x0, x1, x2: base 0x9AC0_2400
        //   = 0x9AC2_2420
        assert_eq!(lsrv_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x9AC2_2420);
    }

    // --- S2-B1 encoder tests ---

    #[test]
    fn fadd_d0_d1_d2_matches_arm_arm() {
        // FADD d0, d1, d2: base 0x1E60_2800
        //   Rm=2 (bits 20-16), Rn=1 (bits 9-5), Rd=0
        //   = 0x1E62_2820
        assert_eq!(fadd_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_2820);
    }

    #[test]
    fn fsub_d0_d1_d2_matches_arm_arm() {
        // FSUB d0, d1, d2: base 0x1E60_3800
        //   = 0x1E62_3820
        assert_eq!(fsub_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_3820);
    }

    #[test]
    fn fmul_d0_d1_d2_matches_arm_arm() {
        // FMUL d0, d1, d2: base 0x1E60_0800
        //   = 0x1E62_0820
        assert_eq!(fmul_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_0820);
    }

    #[test]
    fn fdiv_d0_d1_d2_matches_arm_arm() {
        // FDIV d0, d1, d2: base 0x1E60_1800
        //   = 0x1E62_1820
        assert_eq!(fdiv_d(Fpr::V0, Fpr::V1, Fpr::V2), 0x1E62_1820);
    }

    #[test]
    fn fmov_d0_from_x9_matches_arm_arm() {
        // FMOV d0, x9: base 0x9E67_0000
        //   Rn=9 (bits 9-5), Rd=0
        //   = 0x9E67_0120
        assert_eq!(fmov_d_from_x(Fpr::V0, Gpr::X9), 0x9E67_0120);
    }

    #[test]
    fn fmov_x0_from_d9_matches_arm_arm() {
        // FMOV x0, d9: base 0x9E66_0000
        //   Rn=9 (bits 9-5), Rd=0
        //   = 0x9E66_0120
        assert_eq!(fmov_x_from_d(Gpr::X0, Fpr::V9), 0x9E66_0120);
    }

    // --- S2-C encoder tests ---

    #[test]
    fn cmp_x9_x10_matches_arm_arm() {
        // CMP x9, x10 = SUBS XZR, x9, x10
        //   base 0xEB00_0000 | (Rm=10 << 16) | (Rn=9 << 5) | 31
        //   = 0xEB0A_013F
        assert_eq!(cmp_reg(Gpr::X9, Gpr::X10), 0xEB0A_013F);
    }

    #[test]
    fn fcmp_d16_d17_matches_arm_arm() {
        // FCMP d16, d17: base 0x1E60_2000
        //   Rm=17 (bits 20-16), Rn=16 (bits 9-5)
        //   = 0x1E60_2000 | (17<<16) | (16<<5)
        //   = 0x1E71_2200
        assert_eq!(fcmp_d(Fpr::V16, Fpr::V17), 0x1E71_2200);
    }

    #[test]
    fn cset_eq_matches_arm_arm() {
        // CSET x0, EQ = CSINC x0, XZR, XZR, NE
        //   base 0x9A9F_07E0 with inverted cond NE=1 at bits 15-12
        //   = 0x9A9F_07E0 | (1 << 12) | 0
        //   = 0x9A9F_17E0
        assert_eq!(cset_cond(Gpr::X0, cond::EQ), 0x9A9F_17E0);
    }

    #[test]
    fn cset_lt_to_x9() {
        // CSET x9, LT = CSINC x9, XZR, XZR, GE
        //   GE = 10 = 0b1010 → at bits 15-12 → 0xA000
        //   base 0x9A9F_07E0 | (10<<12) | 9 = 0x9A9F_A7E9
        assert_eq!(cset_cond(Gpr::X9, cond::LT), 0x9A9F_A7E9);
    }
}
