//! Loads + stores — imm12-scaled, register-indexed, and pair forms.

use crate::reg::Gpr;

/// LDR Xt, [Xn, #imm12] — load 64-bit, unsigned offset, scaled by 8.
/// ARM ARM C6.2.131.
pub fn ldr_x_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xF940_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
}

/// STR Xt, [Xn, #imm12] — store 64-bit, unsigned offset, scaled by 8.
/// ARM ARM C6.2.395.
pub fn str_x_imm12(rt: Gpr, rn: Gpr, byte_offset: u32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm12 = byte_offset / 8;
    debug_assert!(imm12 < 4096, "scaled offset must fit in 12 bits");
    0xF900_0000 | (imm12 << 10) | (rn.idx() << 5) | rt.idx()
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
    let imm7 = ((byte_offset / 8) as i32 & 0x7F) as u32;
    0xA980_0000 | (imm7 << 15) | (rt2.idx() << 10) | (rn.idx() << 5) | rt.idx()
}

/// LDP Xt, Xt2, [Xn], #byte_offset — post-index load-pair (64-bit).
/// ARM ARM C6.2.151. Mirror of `stp_pre_index` for the epilogue.
pub fn ldp_post_index(rt: Gpr, rt2: Gpr, rn: Gpr, byte_offset: i32) -> u32 {
    debug_assert!(byte_offset % 8 == 0, "byte offset must be 8-byte aligned");
    let imm7 = ((byte_offset / 8) as i32 & 0x7F) as u32;
    0xA8C0_0000 | (imm7 << 15) | (rt2.idx() << 10) | (rn.idx() << 5) | rt.idx()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
