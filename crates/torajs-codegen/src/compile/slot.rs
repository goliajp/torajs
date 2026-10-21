//! SP-relative slot addressing — spill slots, callee-save slots,
//! stack-param homes, alloca addresses.
//!
//! The scaled-imm12 LDR/STR forms reach 32 KiB below SP and the ADD
//! immediate 4 KiB; a frame past either (a class body with five
//! thousand private fields — test262's `start-unicode-*-class`
//! family — spilled 34–66 KiB) used to be refused outright by a
//! prologue assert, a cap the hardware never imposed. Past the
//! immediate range the access rides a two-instruction form instead:
//! the high 12-bit-shifted part of the offset goes into IP0 (`ADD
//! x16, sp, #hi, LSL #12`) and the low part stays an immediate on
//! that base (`LDR xt, [x16, #lo]`). x16 is the procedure-call
//! scratch register AAPCS64 reserves for exactly this kind of
//! intra-sequence use: nothing in this backend holds a value in it,
//! and no call can intervene between the two words. An alloca
//! address past 4 KiB is the same split as two ADDs.
//!
//! The frame's own carve (`SUB sp, sp, #size`) already took the
//! shifted-pair form; the bound that remains is the pair's 16 MiB.

use super::write_u32;
use crate::enc::{
    add_imm, add_imm_lsl12, ldr_d_imm12, ldr_q_imm12, ldr_x_imm12, str_d_imm12, str_q_imm12,
    str_x_imm12,
};
use crate::reg::{Fpr, Gpr};

/// Intra-sequence base for a slot past the scaled-imm12 range.
const SLOT_BASE: Gpr = Gpr::X16;

/// The largest frame the shifted-immediate pair can carve
/// (`#hi, LSL #12` with hi < 4096): 16 MiB.
pub(crate) const FRAME_BYTES_MAX: u32 = 1 << 24;

/// The largest byte offset the scaled X-form imm12 encodes past its
/// base: 4095 eight-byte units.
pub(crate) const X_IMM12_MAX: u32 = 4095 * 8;

/// `(base, imm)` for a slot at `off` below SP: SP itself while the
/// scaled imm12 reaches, else IP0 loaded with the shifted high part
/// and the low part as the immediate. Slots are 8-aligned, so the low
/// part always fits the scaled form.
fn base_for(bytes: &mut Vec<u8>, off: u32) -> (Gpr, u32) {
    if off < (1 << 15) {
        return (Gpr::SP, off);
    }
    debug_assert!(
        off < FRAME_BYTES_MAX,
        "slot offset {off} past the 16 MiB frame bound"
    );
    write_u32(bytes, add_imm_lsl12(SLOT_BASE, Gpr::SP, (off >> 12) as u16));
    (SLOT_BASE, off & 0xFFF)
}

/// `LDR Xt, [sp, #off]`.
pub(crate) fn sp_ldr_x(bytes: &mut Vec<u8>, rt: Gpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, ldr_x_imm12(rt, base, imm));
}

/// `STR Xt, [sp, #off]`.
pub(crate) fn sp_str_x(bytes: &mut Vec<u8>, rt: Gpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, str_x_imm12(rt, base, imm));
}

/// `LDR Dt, [sp, #off]`.
pub(crate) fn sp_ldr_d(bytes: &mut Vec<u8>, rt: Fpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, ldr_d_imm12(rt, base, imm));
}

/// `STR Dt, [sp, #off]`.
pub(crate) fn sp_str_d(bytes: &mut Vec<u8>, rt: Fpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, str_d_imm12(rt, base, imm));
}

/// `LDR Qt, [sp, #off]`, `off` 16-aligned.
pub(crate) fn sp_ldr_q(bytes: &mut Vec<u8>, rt: Fpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, ldr_q_imm12(rt, base, imm));
}

/// `STR Qt, [sp, #off]`, `off` 16-aligned.
pub(crate) fn sp_str_q(bytes: &mut Vec<u8>, rt: Fpr, off: u32) {
    let (base, imm) = base_for(bytes, off);
    write_u32(bytes, str_q_imm12(rt, base, imm));
}

/// `ADD Xd, sp, #off` — an alloca's address. Past the ADD immediate's
/// 4 KiB the offset splits into the shifted high part and a second
/// ADD for the low part, on `rd` itself.
pub(crate) fn sp_add(bytes: &mut Vec<u8>, rd: Gpr, off: u32) {
    if off < 4096 {
        write_u32(bytes, add_imm(rd, Gpr::SP, off as u16));
        return;
    }
    debug_assert!(
        off < FRAME_BYTES_MAX,
        "alloca offset {off} past the 16 MiB frame bound"
    );
    write_u32(bytes, add_imm_lsl12(rd, Gpr::SP, (off >> 12) as u16));
    let lo = off & 0xFFF;
    if lo > 0 {
        write_u32(bytes, add_imm(rd, rd, lo as u16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// Inside the scaled range the slot form is the one word it
    /// always was: `LDR x0, [sp, #32760]` (imm12 = 4095).
    #[test]
    fn a_slot_inside_the_scaled_range_is_one_word() {
        let mut b = Vec::new();
        sp_ldr_x(&mut b, Gpr::X0, 32760);
        assert_eq!(words(&b), vec![0xF97F_FFE0]);
    }

    /// Past it: `ADD x16, sp, #8, LSL #12` then `LDR x0, [x16, #16]`
    /// for offset 0x8010 — hand-assembled words, not the encoders'.
    #[test]
    fn a_slot_past_the_scaled_range_rides_ip0() {
        let mut b = Vec::new();
        sp_ldr_x(&mut b, Gpr::X0, 0x8010);
        assert_eq!(words(&b), vec![0x9140_23F0, 0xF940_0A00]);
        let mut s = Vec::new();
        sp_str_d(&mut s, Fpr::V0, 0x8010);
        assert_eq!(words(&s), vec![0x9140_23F0, 0xFD00_0A00]);
    }

    /// `str q5, [sp, #32]` / `ldr q5, [sp, #32]` (clang's words), and
    /// the IP0 form past the scaled range.
    #[test]
    fn a_vector_slot_moves_sixteen_bytes() {
        let mut b = Vec::new();
        sp_str_q(&mut b, Fpr::V5, 32);
        sp_ldr_q(&mut b, Fpr::V5, 32);
        assert_eq!(words(&b), vec![0x3D80_0BE5, 0x3DC0_0BE5]);
        let mut far = Vec::new();
        sp_str_q(&mut far, Fpr::V0, 0x8010);
        assert_eq!(words(&far), vec![0x9140_23F0, 0x3D80_0600]);
    }

    /// An alloca address past the ADD immediate: `ADD x1, sp, #1,
    /// LSL #12` then `ADD x1, x1, #8` for offset 4104; exactly 4096
    /// needs no second word.
    #[test]
    fn an_alloca_address_past_4k_is_two_adds() {
        let mut b = Vec::new();
        sp_add(&mut b, Gpr::X1, 4104);
        assert_eq!(words(&b), vec![0x9140_07E1, 0x9100_2021]);
        let mut c = Vec::new();
        sp_add(&mut c, Gpr::X1, 4096);
        assert_eq!(words(&c), vec![0x9140_07E1]);
    }
}
