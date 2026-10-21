//! Loads + stores — imm12-scaled, register-indexed, and pair forms.

use crate::reg::Gpr;

/// LDR Xt, [Xn, #imm12] — load 64-bit, unsigned offset, scaled by 8.
/// ARM ARM C6.2.131.
pub fn ldr_x_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(
        byte_offset.is_multiple_of(8),
        "byte offset must be 8-byte aligned"
    );
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xF940_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// STR Xt, [Xn, #imm12] — store 64-bit, unsigned offset, scaled by 8.
/// ARM ARM C6.2.395.
pub fn str_x_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(
        byte_offset.is_multiple_of(8),
        "byte offset must be 8-byte aligned"
    );
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xF900_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// STR Wt, [Xn, #imm12] — store 32-bit, unsigned offset, scaled by 4.
/// ARM ARM C6.2.395.
pub fn str_w_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(
        byte_offset.is_multiple_of(4),
        "byte offset must be 4-byte aligned"
    );
    let imm12 = byte_offset / 4;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xB900_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDR Wt, [Xn, #imm12] — load 32-bit zero-extended, unsigned
/// offset, scaled by 4. ARM ARM C6.2.131.
pub fn ldr_w_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(
        byte_offset.is_multiple_of(4),
        "byte offset must be 4-byte aligned"
    );
    let imm12 = byte_offset / 4;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xB940_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDUR Wt, [Xn, #imm9] — load 32-bit zero-extended, unscaled
/// signed offset. ARM ARM C6.2.139.
pub fn ldur_w_imm9(rt: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        (-256..=255).contains(&byte_offset),
        "LDUR imm9 must be in -256..=255 (got {byte_offset})"
    );
    let imm9 = (byte_offset as u32) & 0x1FF;
    0xB840_0000 | (imm9 << 12) | (rn.idx() << 5) | rt.idx()
}

/// STUR Wt, [Xn, #imm9] — store 32-bit, unscaled signed offset.
/// ARM ARM C6.2.399.
pub fn stur_w_imm9(rt: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        (-256..=255).contains(&byte_offset),
        "STUR imm9 must be in -256..=255 (got {byte_offset})"
    );
    let imm9 = (byte_offset as u32) & 0x1FF;
    0xB800_0000 | (imm9 << 12) | (rn.idx() << 5) | rt.idx()
}

/// STURH Wt, [Xn, #imm9] — store the low halfword, unscaled signed
/// offset. ARM ARM C6.2.400.
pub fn sturh_w_imm9(rt: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        (-256..=255).contains(&byte_offset),
        "STURH imm9 must be in -256..=255 (got {byte_offset})"
    );
    let imm9 = (byte_offset as u32) & 0x1FF;
    0x7800_0000 | (imm9 << 12) | (rn.idx() << 5) | rt.idx()
}

/// LDR Wt, [Xn, Xm, LSL #0] — load 32-bit zero-extended,
/// register-indexed. ARM ARM C6.2.137.
pub fn ldr_w_reg(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xB860_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// STR Wt, [Xn, Xm, LSL #0] — store 32-bit, register-indexed.
/// ARM ARM C6.2.397.
pub fn str_w_reg(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xB820_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// LDR Xt, [Xn, Xm, LSL #0] — load 64-bit, register-indexed, no
/// scale (Xm is a raw byte index). ARM ARM C6.2.137.
pub fn ldr_x_reg(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xF860_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// STR Xt, [Xn, Xm, LSL #0] — store 64-bit, register-indexed.
/// ARM ARM C6.2.397.
pub fn str_x_reg(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xF820_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// LDRB Wt, [Xn, Xm, LSL #0] — load ONE byte, register-indexed,
/// zero-extended into Wt (which zeroes the upper 32 bits of Xt).
/// ARM ARM C6.2.147 (LDRB register). clang: `ldrb w0, [x1, x2]`
/// = 0x3862_6820.
pub fn ldrb_w_reg(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x3860_6800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// LDRB Wt, [Xn, #imm12] — load one byte, unsigned offset (unscaled
/// for a byte), zero-extended into Wt. clang: `ldrb w10, [x9,
/// #1153]` = 0x3952_052A.
pub fn ldrb_w_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset < 4096, "offset must fit in 12 bits");
    0x3940_0000 | (byte_offset << 10) | (rn.idx() << 5) | rt.idx()
}

/// STRB Wt, [Xn, #imm12] — store the low byte of Wt, unsigned offset.
/// clang: `strb w10, [x9, #1153]` = 0x3912_052A.
pub fn strb_w_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset < 4096, "offset must fit in 12 bits");
    0x3900_0000 | (byte_offset << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDRH Wt, [Xn, Xm, LSL #1] — load one halfword at `Xn + Xm*2`,
/// zero-extended into Wt (S = 1 folds the scale into the AGU).
/// ARM ARM C6.2.157 (LDRH register). clang: `ldrh w0, [x1, x2,
/// lsl #1]` = 0x7862_7820.
pub fn ldrh_w_reg_lsl1(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0x7860_7800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// LDRH Wt, [Xn, #imm12 * 2] — load one halfword, unsigned offset
/// scaled by 2, zero-extended into Wt. clang: `ldrh w0, [x1, #16]` =
/// 0x7940_2020.
pub fn ldrh_w_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(
        byte_offset.is_multiple_of(2) && byte_offset < 8192,
        "offset must be an even byte count below 8 KiB"
    );
    0x7940_0000 | ((byte_offset / 2) << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDR Xt, [Xn, Xm, LSL #3] — load 64-bit, register-indexed with
/// the scale folded into the AGU (S = 1). clang: `ldr x0, [x1, x2,
/// lsl #3]` = 0xF862_7820.
pub fn ldr_x_reg_lsl3(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xF860_7800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// STR Xt, [Xn, Xm, LSL #3] — store 64-bit, register-indexed scaled.
/// clang: `str x0, [x1, x2, lsl #3]` = 0xF822_7820.
pub fn str_x_reg_lsl3(rt: Gpr, rn: Gpr, rm: Gpr) -> u32 {
    0xF820_7800 | (rm.idx() << 16) | (rn.idx() << 5) | rt.idx()
}

/// LDUR Xt, [Xn, #imm9] — load 64-bit, **unscaled** signed 9-bit
/// offset (range -256..=255). Use when the offset is not a multiple
/// of 8 — the scaled LDR form requires 8-byte alignment. ARM ARM
/// C6.2.139.
pub fn ldur_x_imm9(rt: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        (-256..=255).contains(&byte_offset),
        "LDUR imm9 must be in -256..=255 (got {byte_offset})"
    );
    let imm9 = (byte_offset as u32) & 0x1FF;
    0xF840_0000 | (imm9 << 12) | (rn.idx() << 5) | rt.idx()
}

/// STUR Xt, [Xn, #imm9] — store 64-bit, **unscaled** signed 9-bit
/// offset. Mirror of [`ldur_x_imm9`]; use when the byte offset is
/// not 8-byte aligned. ARM ARM C6.2.399.
pub fn stur_x_imm9(rt: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        (-256..=255).contains(&byte_offset),
        "STUR imm9 must be in -256..=255 (got {byte_offset})"
    );
    let imm9 = (byte_offset as u32) & 0x1FF;
    0xF800_0000 | (imm9 << 12) | (rn.idx() << 5) | rt.idx()
}

/// STP Xt, Xt2, [Xn, #byte_offset]! — pre-index store-pair (64-bit).
/// ARM ARM C6.2.385. `byte_offset` is signed, 8-aligned, ±504 range.
/// Used in prologue as `STP x29, x30, [SP, #-16]!`.
pub fn stp_pre_index(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm7 = ((byte_offset / 8) & 0x7F) as u32;
    0xA980_0000 | (imm7 << 15) | (rt2.idx() << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDP Xt, Xt2, [Xn], #byte_offset — post-index load-pair (64-bit).
/// ARM ARM C6.2.151. Mirror of `stp_pre_index` for the epilogue.
pub fn ldp_post_index(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm7 = ((byte_offset / 8) & 0x7F) as u32;
    0xA8C0_0000 | (imm7 << 15) | (rt2.idx() << 10) | (rn.idx() << 5) | rt.idx()
}

/// Signed-offset load / store pair, base form: `imm7` is the byte
/// offset scaled by 8, so the offset must be 8-aligned in -512..=504.
fn pair_off(base: u32, rt: u32, rt2: u32, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        byte_offset % 8 == 0 && (-512..=504).contains(&byte_offset),
        "pair offset must be 8-aligned in -512..=504 (got {byte_offset})"
    );
    let imm7 = ((byte_offset / 8) as u32) & 0x7F;
    base | (imm7 << 15) | (rt2 << 10) | (rn.idx() << 5) | rt
}

/// STP Xt, Xt2, [Xn, #byte_offset] — ARM ARM C6.2.385 (signed offset).
pub fn stp_x_off(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    pair_off(0xA900_0000, rt.idx(), rt2.idx(), rn, byte_offset)
}

/// STP Wt, Wt2, [Xn, #byte_offset] — the 32-bit form: `imm7` scales
/// by 4, so the offset must be 4-aligned in -256..=252.
pub fn stp_w_off(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    assert!(
        byte_offset % 4 == 0 && (-256..=252).contains(&byte_offset),
        "pair offset must be 4-aligned in -256..=252 (got {byte_offset})"
    );
    let imm7 = ((byte_offset / 4) as u32) & 0x7F;
    0x2900_0000 | (imm7 << 15) | (rt2.idx() << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDP Xt, Xt2, [Xn, #byte_offset] — ARM ARM C6.2.151 (signed offset).
pub fn ldp_x_off(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    pair_off(0xA940_0000, rt.idx(), rt2.idx(), rn, byte_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_w_matches_assembler() {
        // `as -arch arm64`: str w10, [x0] / str w10, [x3, #8]
        assert_eq!(str_w_imm12(Gpr::X10, Gpr::X0, 0), 0xB900_000A);
        assert_eq!(str_w_imm12(Gpr::X10, Gpr::X3, 8), 0xB900_086A);
    }

    #[test]
    fn w_forms_match_clang() {
        // clang -c: ldr w0, [x1, #4] / ldr w9, [x13, #8] / ldur w0, [x1, #6]
        // stur w0, [x1, #-2] / ldr w0, [x1, x2] / str w0, [x1, x2]
        assert_eq!(ldr_w_imm12(Gpr::X0, Gpr::X1, 4), 0xB940_0420);
        assert_eq!(ldr_w_imm12(Gpr::X9, Gpr::X13, 8), 0xB940_09A9);
        assert_eq!(ldur_w_imm9(Gpr::X0, Gpr::X1, 6), 0xB840_6020);
        assert_eq!(stur_w_imm9(Gpr::X0, Gpr::X1, -2), 0xB81F_E020);
        assert_eq!(ldr_w_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xB862_6820);
        assert_eq!(str_w_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0xB822_6820);
        assert_eq!(str_w_imm12(Gpr::X0, Gpr::X1, 4), 0xB900_0420);
    }

    #[test]
    fn store_pairs_match_clang() {
        // clang -c: stp w9, w9, [x13] / stp w9, w11, [x6, #8]
        // stp x9, xzr, [x13, #8] / stp xzr, x9, [x0, #16]
        assert_eq!(stp_w_off(Gpr::X9, Gpr::X9, Gpr::X13, 0), 0x2900_25A9);
        assert_eq!(stp_w_off(Gpr::X9, Gpr::X11, Gpr::X6, 8), 0x2901_2CC9);
        assert_eq!(stp_x_off(Gpr::X9, Gpr::XZR, Gpr::X13, 8), 0xA900_FDA9);
        assert_eq!(stp_x_off(Gpr::XZR, Gpr::X9, Gpr::X0, 16), 0xA901_241F);
    }

    #[test]
    fn ldr_x0_x9_offset_zero_matches_arm_arm() {
        assert_eq!(ldr_x_imm12(Gpr::X0, Gpr::X9, 0), 0xF940_0120);
    }

    #[test]
    fn ldr_with_scaled_offset() {
        // LDR x0, [x9, #16] — imm12=2
        assert_eq!(ldr_x_imm12(Gpr::X0, Gpr::X9, 16), 0xF940_0920);
    }

    #[test]
    fn str_x9_x12_offset_zero_matches_arm_arm() {
        assert_eq!(str_x_imm12(Gpr::X9, Gpr::X12, 0), 0xF900_0189);
    }

    #[test]
    fn ldr_x_reg_x0_x12_x10_matches_arm_arm() {
        assert_eq!(ldr_x_reg(Gpr::X0, Gpr::X12, Gpr::X10), 0xF86A_6980);
    }

    #[test]
    fn ldrb_w_reg_w0_x1_x2_matches_clang() {
        // clang -c: `ldrb w0, [x1, x2]` = 0x3862_6820
        assert_eq!(ldrb_w_reg(Gpr::X0, Gpr::X1, Gpr::X2), 0x3862_6820);
    }

    #[test]
    fn byte_imm12_forms_match_clang() {
        // clang -c: ldrb w10, [x9, #1153] / strb w10, [x9, #1153]
        // ldrb w0, [x1] / strb w0, [x1]
        assert_eq!(ldrb_w_imm12(Gpr::X10, Gpr::X9, 1153), 0x3952_052A);
        assert_eq!(strb_w_imm12(Gpr::X10, Gpr::X9, 1153), 0x3912_052A);
        assert_eq!(ldrb_w_imm12(Gpr::X0, Gpr::X1, 0), 0x3940_0020);
        assert_eq!(strb_w_imm12(Gpr::X0, Gpr::X1, 0), 0x3900_0020);
    }

    #[test]
    fn ldrh_w_imm12_w0_x1_16_matches_clang() {
        assert_eq!(ldrh_w_imm12(Gpr::X0, Gpr::X1, 16), 0x7940_2020);
    }

    #[test]
    fn ldrh_w_reg_lsl1_w0_x1_x2_matches_clang() {
        // clang -c: `ldrh w0, [x1, x2, lsl #1]` = 0x7862_7820
        assert_eq!(ldrh_w_reg_lsl1(Gpr::X0, Gpr::X1, Gpr::X2), 0x7862_7820);
    }

    #[test]
    fn str_x_reg_x9_x12_x11_matches_arm_arm() {
        assert_eq!(str_x_reg(Gpr::X9, Gpr::X12, Gpr::X11), 0xF82B_6989);
    }

    #[test]
    fn stp_x29_x30_pre_index_minus_16_matches_clang() {
        // Canonical leaf-fn prologue `STP x29, x30, [SP, #-16]!`
        assert_eq!(stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16), 0xA9BF_7BFD);
    }

    #[test]
    fn stur_x_imm9_unaligned_offset_4_matches_arm_arm() {
        // STUR x9, [x12, #4] — imm9=4, encodes when offset isn't
        // multiple of 8 (where scaled STR would silently misencode
        // via floor-divide-by-8).
        assert_eq!(stur_x_imm9(Gpr::X9, Gpr::X12, 4), 0xF800_4189);
    }

    #[test]
    fn sturh_w_imm9_odd_offset_matches_arm_arm() {
        // STURH w12, [x11, #17]
        assert_eq!(sturh_w_imm9(Gpr::X12, Gpr::X11, 17), 0x7801_116C);
    }

    #[test]
    fn ldur_x_imm9_unaligned_offset_4_matches_arm_arm() {
        // LDUR x9, [x12, #4] — mirror of stur_x_imm9 test.
        assert_eq!(ldur_x_imm9(Gpr::X9, Gpr::X12, 4), 0xF840_4189);
    }

    #[test]
    fn stur_x_imm9_negative_offset_minus_8() {
        // Sanity: negative offsets sign-extend via the 9-bit field.
        let word = stur_x_imm9(Gpr::X0, Gpr::X1, -8);
        // imm9 = -8 mod 512 = 0x1F8
        assert_eq!(word, 0xF800_0000 | (0x1F8 << 12) | (1 << 5));
    }

    #[test]
    fn ldp_x29_x30_post_index_16_matches_clang() {
        // Canonical leaf-fn epilogue `LDP x29, x30, [SP], #16`
        assert_eq!(ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16), 0xA8C1_7BFD);
    }

    #[test]
    fn signed_offset_pairs_match_clang() {
        // clang -c: stp x19, x20, [sp, #16] / ldp … / stp x27, x28, [sp, #-512]
        assert_eq!(stp_x_off(Gpr::X19, Gpr::X20, Gpr::SP, 16), 0xA901_53F3);
        assert_eq!(ldp_x_off(Gpr::X19, Gpr::X20, Gpr::SP, 16), 0xA941_53F3);
        assert_eq!(stp_x_off(Gpr::X27, Gpr::X28, Gpr::SP, -512), 0xA920_73FB);
    }
}
