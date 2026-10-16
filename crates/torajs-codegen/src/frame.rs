//! AAPCS64 stack frame protocol.
//!
//! S1-S2: leaf-function-only — no Alloca, no calls, no spills, no
//! callee-save touched. For such fns the prologue / epilogue are
//! empty and the body runs in the caller's frame.
//!
//! S3-A2: Alloca lands. When the function has any `Alloca` /
//! `AllocaBytes`, we carve a 16-aligned region below saved FP/LR:
//!
//! ```text
//!   high addr
//!   ┌───────────────────────────┐  ← caller sp
//!   │  saved x29 (frame ptr)    │
//!   │  saved x30 (link reg)     │
//!   ├───────────────────────────┤  ← new x29 (frame ptr) ↓
//!   │  alloca region            │  ← `alloca_bytes` (16-aligned)
//!   └───────────────────────────┘  ← new sp
//!   low addr
//! ```
//!
//! Prologue:
//!   STP x29, x30, [sp, #-16]!     # save FP+LR, sp -= 16
//!   MOV x29, sp                   # FP = sp = saved-FP/LR location
//!   SUB sp, sp, #alloca_bytes     # carve alloca region (if > 0)
//!
//! Epilogue (reverse, runs immediately before RET):
//!   ADD sp, sp, #alloca_bytes     # release alloca region
//!   LDP x29, x30, [sp], #16       # restore FP+LR, sp += 16
//!   RET                           # (emitted by emit_terminator)
//!
//! Slot addressing: the K-th alloca's byte offset is its cumulative
//! position in the alloca region (slot 0 at `sp+0`, slot 1 at
//! `sp+slot0_size`, etc). `regalloc::Assignment::alloca_offset_of`
//! reports this offset.
//!
//! Spill slots / callee-save register handling lands in S3-B+ along
//! with full Linear Scan; this S3-A2 implementation only covers the
//! Alloca-region carve-out.

use crate::compile::slot;
use crate::enc::{
    add_imm, add_imm_lsl12, ldp_post_index, ldp_x_off, movz_imm, stp_pre_index, stp_x_off, sub_imm,
    sub_imm_lsl12,
};
use crate::reg::{Fpr, Gpr, aapcs64};

/// Largest byte offset a signed-offset STP / LDP pair encodes (imm7 × 8).
const PAIR_OFF_MAX: u32 = 504;

/// Layout descriptor for a function's stack frame.
///
/// Memory layout (high → low address, after the prologue):
///
/// ```text
///   ┌───────────────────────────┐  ← caller sp
///   │  saved x29 / x30          │  STP, sp -= 16  → new x29
///   ├───────────────────────────┤
///   │  callee-save area         │  offsets [alloca_bytes, +cs_bytes)
///   ├───────────────────────────┤
///   │  spill region             │  ┐ offsets [0, alloca_bytes); the
///   │  alloca region            │  ┘ SSA-visible spill/alloca offsets
///   └───────────────────────────┘  ← new sp   SUB sp, #frame_size
/// ```
///
/// The callee-save area sits *above* the alloca/spill region so the
/// SSA-visible offsets (carved by `linear_scan` as `sp + off`) are
/// unaffected by how many callee-saved registers a call-crossing
/// allocation happened to need.
#[derive(Debug, Clone, Copy)]
pub struct FrameLayout {
    /// alloca + spill region bytes, 16-byte aligned per AAPCS64
    /// sp-alignment. The callee-save area is stacked above this.
    pub alloca_bytes: u32,
    /// Bitmask of the callee-saved GPRs (`Gpr::idx`) this function
    /// must save/restore — the subset of `aapcs64::CALLEE_SAVED_SCRATCH`
    /// the allocator handed to call-crossing values. 0 for leaf /
    /// non-crossing functions.
    pub callee_saved_gpr_mask: u32,
    /// Bitmask of the callee-saved FPRs (`Fpr::idx`, from
    /// `aapcs64::FP_CALLEE_SAVED_SCRATCH`) this function must
    /// save/restore (D-form, low 64 bits — AAPCS64 §5.1.2).
    pub callee_saved_fpr_mask: u32,
    /// True if the function makes any calls (any `BL`/`BLR` site) —
    /// forces FP/LR save in the prologue.
    pub uses_calls: bool,
    /// The prologue ends by zeroing this register, which the
    /// callee-save area saves (the throw register of a guarded reader,
    /// compile/throw_reg.rs).
    pub zeroed: Option<Gpr>,
}

impl FrameLayout {
    /// Trivial layout — no alloca, no calls, no callee-saves. S1-S2
    /// default.
    pub const fn leaf_no_spill() -> Self {
        Self {
            alloca_bytes: 0,
            callee_saved_gpr_mask: 0,
            callee_saved_fpr_mask: 0,
            uses_calls: false,
            zeroed: None,
        }
    }

    /// Build a frame layout from raw alloca+spill byte count with no
    /// callee-saved registers. Rounds up to the 16-byte sp-alignment.
    pub fn from_alloca_bytes(raw_alloca_bytes: u32, uses_calls: bool) -> Self {
        Self::with_callee_saved(raw_alloca_bytes, 0, 0, uses_calls)
    }

    /// Build a frame layout including a callee-save area. `gpr_mask` /
    /// `fpr_mask` are the `Gpr::idx` / `Fpr::idx` bitmasks of the
    /// callee-saved registers the allocator used for call-crossing
    /// values.
    pub fn with_callee_saved(
        raw_alloca_bytes: u32,
        callee_saved_gpr_mask: u32,
        callee_saved_fpr_mask: u32,
        uses_calls: bool,
    ) -> Self {
        let aligned = (raw_alloca_bytes + 15) & !15;
        Self {
            alloca_bytes: aligned,
            callee_saved_gpr_mask,
            callee_saved_fpr_mask,
            uses_calls,
            zeroed: None,
        }
    }

    /// Bytes occupied by the callee-save area (8 per saved register,
    /// pre-alignment).
    fn callee_saved_bytes(&self) -> u32 {
        (self.callee_saved_gpr_mask.count_ones() + self.callee_saved_fpr_mask.count_ones()) * 8
    }

    /// Total sp adjustment the prologue carves = alloca/spill region +
    /// callee-save area, rounded up to 16. `alloca_bytes` is already
    /// 16-aligned and callee-save bytes are 8-aligned, so this adds at
    /// most one 8-byte pad at the top of the callee-save area.
    pub fn frame_size(&self) -> u32 {
        (self.alloca_bytes + self.callee_saved_bytes() + 15) & !15
    }

    /// `true` when no prologue/epilogue code is needed.
    pub const fn is_trivial(&self) -> bool {
        self.alloca_bytes == 0
            && !self.uses_calls
            && self.callee_saved_gpr_mask == 0
            && self.callee_saved_fpr_mask == 0
    }

    /// Emit the callee-save area stores (prologue) or loads
    /// (epilogue). Each used register gets an 8-byte slot stacked above
    /// the alloca/spill region (base = `alloca_bytes`), in
    /// `CALLEE_SAVED_SCRATCH` / `FP_CALLEE_SAVED_SCRATCH` order; two
    /// neighbouring GPRs share an STP / LDP while the pair's offset
    /// fits the instruction, everything else takes single STR / LDR.
    fn emit_callee_saves(&self, bytes: &mut Vec<u8>, restore: bool) {
        let gprs: Vec<Gpr> = aapcs64::CALLEE_SAVED_SCRATCH
            .iter()
            .copied()
            .filter(|g| self.callee_saved_gpr_mask & (1 << g.idx()) != 0)
            .collect();
        let fprs: Vec<Fpr> = aapcs64::FP_CALLEE_SAVED_SCRATCH
            .iter()
            .copied()
            .filter(|v| self.callee_saved_fpr_mask & (1 << v.idx()) != 0)
            .collect();
        let mut off = self.alloca_bytes;
        for pair in gprs.chunks(2) {
            match pair {
                [a, b] if off <= PAIR_OFF_MAX => {
                    let word = if restore { ldp_x_off } else { stp_x_off };
                    write_word(bytes, word(*a, *b, Gpr::SP, off as i32));
                }
                _ => {
                    for (k, g) in pair.iter().enumerate() {
                        let at = off + 8 * k as u32;
                        if restore {
                            slot::sp_ldr_x(bytes, *g, at);
                        } else {
                            slot::sp_str_x(bytes, *g, at);
                        }
                    }
                }
            }
            off += 8 * pair.len() as u32;
        }
        // FP registers stay single: on Apple cores an 8-byte FP reload
        // of half an `stp d, d` pair misses store-to-load forwarding,
        // and the early-return path of a small recursive function hits
        // it on every call (fib40 +37% measured).
        for v in fprs {
            if restore {
                slot::sp_ldr_d(bytes, v, off);
            } else {
                slot::sp_str_d(bytes, v, off);
            }
            off += 8;
        }
    }

    /// Emit the prologue into `bytes`. No-op when trivial.
    pub fn emit_prologue(&self, bytes: &mut Vec<u8>) {
        if self.is_trivial() {
            return;
        }
        // STP x29, x30, [sp, #-16]!  — save FP/LR, sp -= 16
        write_word(bytes, stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16));
        // MOV x29, sp = ADD x29, sp, #0  — FP anchors at saved-FP location
        write_word(bytes, add_imm(Gpr::X29, Gpr::SP, 0));
        // SUB sp, sp, #frame_size  — carve alloca + spill + callee-save.
        // Sizes past imm12 split into a shifted-imm pair
        // (SUB #hi, LSL #12 + SUB #lo). Slot accesses past the
        // scaled-imm12 range take the IP0-based form (`compile::slot`,
        // rotation 626 — the 32 KiB cap that stood here refused a
        // class body with five thousand private fields); what remains
        // is the pair's own 16 MiB.
        let total = self.frame_size();
        if total > 0 {
            assert!(
                total < crate::compile::slot::FRAME_BYTES_MAX,
                "frame size {total} exceeds the 16 MiB shifted-immediate frame bound"
            );
            let hi = total >> 12;
            let lo = total & 0xFFF;
            if hi > 0 {
                write_word(bytes, sub_imm_lsl12(Gpr::SP, Gpr::SP, hi as u16));
            }
            if lo > 0 {
                write_word(bytes, sub_imm(Gpr::SP, Gpr::SP, lo as u16));
            }
        }
        // Save callee-saved registers into the area above alloca/spill.
        self.emit_callee_saves(bytes, /*restore=*/ false);
        if let Some(g) = self.zeroed {
            write_word(bytes, movz_imm(g, 0, 0));
        }
    }

    /// Emit the epilogue into `bytes`. No-op when trivial. RET is
    /// emitted separately by `compile::emit_terminator` immediately
    /// after this.
    ///
    /// The frame record comes back first whenever its slot is in reach
    /// of a signed-offset LDP (the order Apple clang emits): the load
    /// of x30 then depends only on the sp the epilogue starts from, so
    /// the return address is ready while the callee-saves still load.
    /// Restoring it last, behind the ADD that releases the frame, left
    /// RET waiting on that chain; a 2000-deep recursion, which has
    /// outrun the return-address predictor, ran 3% slower for it.
    pub fn emit_epilogue(&self, bytes: &mut Vec<u8>) {
        if self.is_trivial() {
            return;
        }
        let total = self.frame_size();
        if total > 0 && total <= PAIR_OFF_MAX {
            write_word(bytes, ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, total as i32));
            self.emit_callee_saves(bytes, /*restore=*/ true);
            write_word(bytes, add_imm(Gpr::SP, Gpr::SP, (total + 16) as u16));
            return;
        }
        // Restore callee-saved registers (offsets still valid — sp is
        // at the bottom of the frame until the ADD below).
        self.emit_callee_saves(bytes, /*restore=*/ true);
        // ADD sp, sp, #frame_size  — release the whole frame
        // (shifted-imm pair past imm12, mirroring the prologue).
        if total > 0 {
            let hi = total >> 12;
            let lo = total & 0xFFF;
            if hi > 0 {
                write_word(bytes, add_imm_lsl12(Gpr::SP, Gpr::SP, hi as u16));
            }
            if lo > 0 {
                write_word(bytes, add_imm(Gpr::SP, Gpr::SP, lo as u16));
            }
        }
        // LDP x29, x30, [sp], #16  — restore FP/LR, sp += 16
        write_word(bytes, ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16));
    }
}

fn write_word(bytes: &mut Vec<u8>, word: u32) {
    bytes.extend_from_slice(&word.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enc::{ldp_x_off, stp_x_off, str_d_imm12, str_x_imm12};
    use crate::reg::Fpr;

    #[test]
    fn leaf_no_spill_is_trivial() {
        let f = FrameLayout::leaf_no_spill();
        assert!(f.is_trivial());
        assert_eq!(f.alloca_bytes, 0);
        assert!(!f.uses_calls);
    }

    #[test]
    fn from_alloca_bytes_rounds_to_16() {
        assert_eq!(FrameLayout::from_alloca_bytes(0, false).alloca_bytes, 0);
        assert_eq!(FrameLayout::from_alloca_bytes(1, false).alloca_bytes, 16);
        assert_eq!(FrameLayout::from_alloca_bytes(8, false).alloca_bytes, 16);
        assert_eq!(FrameLayout::from_alloca_bytes(16, false).alloca_bytes, 16);
        assert_eq!(FrameLayout::from_alloca_bytes(17, false).alloca_bytes, 32);
        assert_eq!(FrameLayout::from_alloca_bytes(48, false).alloca_bytes, 48);
    }

    #[test]
    fn nonzero_frame_is_not_trivial() {
        let f = FrameLayout::from_alloca_bytes(16, false);
        assert!(!f.is_trivial());
    }

    /// Frame past imm12 (RC-4 F1a fallout: guard-heavy fixtures hit
    /// 7184) carves and releases sp with the shifted-imm pair:
    /// SUB #hi LSL#12 + SUB #lo, mirrored in the epilogue.
    #[test]
    fn prologue_epilogue_large_frame_shifted_imm_pair() {
        let f = FrameLayout::from_alloca_bytes(7184, true);
        assert_eq!(f.frame_size(), 7184);

        let mut prologue = Vec::new();
        f.emit_prologue(&mut prologue);
        let expected_pro = [
            stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            add_imm(Gpr::X29, Gpr::SP, 0),
            sub_imm_lsl12(Gpr::SP, Gpr::SP, 1), // 4096
            sub_imm(Gpr::SP, Gpr::SP, 3088),    // 7184 - 4096
        ];
        let mut want = Vec::new();
        for w in expected_pro {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(prologue, want);

        let mut epilogue = Vec::new();
        f.emit_epilogue(&mut epilogue);
        let expected_epi = [
            add_imm_lsl12(Gpr::SP, Gpr::SP, 1),
            add_imm(Gpr::SP, Gpr::SP, 3088),
            ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
        ];
        let mut want_epi = Vec::new();
        for w in expected_epi {
            want_epi.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(epilogue, want_epi);
    }

    /// An exact multiple of 4096 emits only the shifted-imm half.
    #[test]
    fn large_frame_exact_multiple_single_shifted_sub() {
        let f = FrameLayout::from_alloca_bytes(8192, true);
        let mut prologue = Vec::new();
        f.emit_prologue(&mut prologue);
        let expected = [
            stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            add_imm(Gpr::X29, Gpr::SP, 0),
            sub_imm_lsl12(Gpr::SP, Gpr::SP, 2),
        ];
        let mut want = Vec::new();
        for w in expected {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(prologue, want);
    }

    #[test]
    fn uses_calls_is_not_trivial_even_with_zero_frame() {
        let f = FrameLayout {
            alloca_bytes: 0,
            callee_saved_gpr_mask: 0,
            callee_saved_fpr_mask: 0,
            uses_calls: true,
            zeroed: None,
        };
        assert!(!f.is_trivial());
    }

    #[test]
    fn prologue_empty_for_trivial_frame() {
        let mut bytes = Vec::new();
        FrameLayout::leaf_no_spill().emit_prologue(&mut bytes);
        assert!(bytes.is_empty());
    }

    #[test]
    fn prologue_16_byte_frame() {
        let mut bytes = Vec::new();
        FrameLayout::from_alloca_bytes(16, false).emit_prologue(&mut bytes);
        let expected_words = [
            stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            add_imm(Gpr::X29, Gpr::SP, 0),
            sub_imm(Gpr::SP, Gpr::SP, 16),
        ];
        let mut expected = Vec::with_capacity(12);
        for w in expected_words {
            expected.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(bytes, expected);
    }

    #[test]
    fn epilogue_16_byte_frame() {
        let mut bytes = Vec::new();
        FrameLayout::from_alloca_bytes(16, false).emit_epilogue(&mut bytes);
        let expected_words = [
            ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            add_imm(Gpr::SP, Gpr::SP, 32),
        ];
        let mut expected = Vec::with_capacity(8);
        for w in expected_words {
            expected.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(bytes, expected);
    }

    /// The frame record comes back first while its slot is in LDP's
    /// signed-offset reach (504); a larger frame keeps the release-
    /// then-post-index order.
    #[test]
    fn epilogue_frame_record_first_within_pair_reach() {
        let mut near = Vec::new();
        FrameLayout::from_alloca_bytes(496, false).emit_epilogue(&mut near);
        assert_eq!(
            near[..4],
            ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, 496).to_le_bytes()
        );
        assert_eq!(near[4..], add_imm(Gpr::SP, Gpr::SP, 512).to_le_bytes());
        let mut far = Vec::new();
        FrameLayout::from_alloca_bytes(512, false).emit_epilogue(&mut far);
        let mut want = add_imm(Gpr::SP, Gpr::SP, 512).to_le_bytes().to_vec();
        want.extend_from_slice(&ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16).to_le_bytes());
        assert_eq!(far, want);
    }

    #[test]
    fn callee_saved_mask_makes_frame_non_trivial() {
        let f = FrameLayout::with_callee_saved(0, 1 << Gpr::X19.idx(), 0, true);
        assert!(!f.is_trivial());
    }

    #[test]
    fn frame_size_stacks_callee_saves_above_alloca() {
        // 1 GPR (8 bytes) + alloca 16 → 24 → rounds to 32.
        let f = FrameLayout::with_callee_saved(16, 1 << Gpr::X19.idx(), 0, true);
        assert_eq!(f.alloca_bytes, 16);
        assert_eq!(f.frame_size(), 32);
        // 2 GPRs (16 bytes) + no alloca → 16.
        let f = FrameLayout::with_callee_saved(
            0,
            (1 << Gpr::X19.idx()) | (1 << Gpr::X20.idx()),
            0,
            true,
        );
        assert_eq!(f.frame_size(), 16);
    }

    /// Two callee-saved GPRs (X19, X20), no alloca: prologue saves
    /// them above the (empty) alloca region at sp+0 / sp+8 after a
    /// 16-byte SUB; epilogue restores then releases.
    #[test]
    fn prologue_epilogue_saves_two_callee_gprs() {
        let mask = (1 << Gpr::X19.idx()) | (1 << Gpr::X20.idx());
        let frame = FrameLayout::with_callee_saved(0, mask, 0, true);

        let mut prologue = Vec::new();
        frame.emit_prologue(&mut prologue);
        let expected_pro = [
            stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            add_imm(Gpr::X29, Gpr::SP, 0),
            sub_imm(Gpr::SP, Gpr::SP, 16),
            stp_x_off(Gpr::X19, Gpr::X20, Gpr::SP, 0),
        ];
        let mut want = Vec::new();
        for w in expected_pro {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(prologue, want);

        let mut epilogue = Vec::new();
        frame.emit_epilogue(&mut epilogue);
        let expected_epi = [
            ldp_x_off(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            ldp_x_off(Gpr::X19, Gpr::X20, Gpr::SP, 0),
            add_imm(Gpr::SP, Gpr::SP, 32),
        ];
        let mut want = Vec::new();
        for w in expected_epi {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(epilogue, want);
    }

    /// Mixed GPR + FPR callee-saves with an alloca region: the FPR
    /// save sits above the GPR save, which sits above the alloca
    /// region. Verifies D-form STR/LDR offsets.
    #[test]
    fn prologue_saves_gpr_then_fpr_above_alloca() {
        // alloca 16, one GPR (X19), one FPR (V8). cs_bytes = 16,
        // total = align16(16 + 16) = 32.
        let frame =
            FrameLayout::with_callee_saved(16, 1 << Gpr::X19.idx(), 1 << Fpr::V8.idx(), true);
        assert_eq!(frame.frame_size(), 32);

        let mut prologue = Vec::new();
        frame.emit_prologue(&mut prologue);
        let expected = [
            stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            add_imm(Gpr::X29, Gpr::SP, 0),
            sub_imm(Gpr::SP, Gpr::SP, 32),
            str_x_imm12(Gpr::X19, Gpr::SP, 16), // GPR slot above alloca[0,16)
            str_d_imm12(Fpr::V8, Gpr::SP, 24),  // FPR slot above the GPR
        ];
        let mut want = Vec::new();
        for w in expected {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(prologue, want);
    }
}
