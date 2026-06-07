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

use crate::enc::{
    add_imm, ldp_post_index, ldr_d_imm12, ldr_x_imm12, stp_pre_index, str_d_imm12, str_x_imm12,
    sub_imm,
};
use crate::reg::{Gpr, aapcs64};

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

    /// Emit the callee-save area STR/LDR pair stream. Each used
    /// register gets an 8-byte slot stacked above the alloca/spill
    /// region (base = `alloca_bytes`), in `CALLEE_SAVED_SCRATCH` /
    /// `FP_CALLEE_SAVED_SCRATCH` order. `restore=false` emits stores
    /// (prologue), `restore=true` loads (epilogue) at the same
    /// offsets — order is irrelevant since every slot is independent.
    fn emit_callee_saves(&self, bytes: &mut Vec<u8>, restore: bool) {
        let mut off = self.alloca_bytes;
        for &g in aapcs64::CALLEE_SAVED_SCRATCH.iter() {
            if self.callee_saved_gpr_mask & (1 << g.idx()) != 0 {
                let w = if restore {
                    ldr_x_imm12(g, Gpr::SP, off)
                } else {
                    str_x_imm12(g, Gpr::SP, off)
                };
                write_word(bytes, w);
                off += 8;
            }
        }
        for &v in aapcs64::FP_CALLEE_SAVED_SCRATCH.iter() {
            if self.callee_saved_fpr_mask & (1 << v.idx()) != 0 {
                let w = if restore {
                    ldr_d_imm12(v, Gpr::SP, off)
                } else {
                    str_d_imm12(v, Gpr::SP, off)
                };
                write_word(bytes, w);
                off += 8;
            }
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
        let total = self.frame_size();
        if total > 0 {
            assert!(
                total < 4096,
                "frame size {total} must fit in ADD/SUB imm12; larger frames land later with shifted-imm or scratch-reg path"
            );
            write_word(bytes, sub_imm(Gpr::SP, Gpr::SP, total as u16));
        }
        // Save callee-saved registers into the area above alloca/spill.
        self.emit_callee_saves(bytes, /*restore=*/ false);
    }

    /// Emit the epilogue into `bytes`. No-op when trivial. RET is
    /// emitted separately by `compile::emit_terminator` immediately
    /// after this.
    pub fn emit_epilogue(&self, bytes: &mut Vec<u8>) {
        if self.is_trivial() {
            return;
        }
        // Restore callee-saved registers (offsets still valid — sp is
        // at the bottom of the frame until the ADD below).
        self.emit_callee_saves(bytes, /*restore=*/ true);
        // ADD sp, sp, #frame_size  — release the whole frame.
        let total = self.frame_size();
        if total > 0 {
            write_word(bytes, add_imm(Gpr::SP, Gpr::SP, total as u16));
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

    #[test]
    fn uses_calls_is_not_trivial_even_with_zero_frame() {
        let f = FrameLayout {
            alloca_bytes: 0,
            callee_saved_gpr_mask: 0,
            callee_saved_fpr_mask: 0,
            uses_calls: true,
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
            add_imm(Gpr::SP, Gpr::SP, 16),
            ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
        ];
        let mut expected = Vec::with_capacity(8);
        for w in expected_words {
            expected.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(bytes, expected);
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
            str_x_imm12(Gpr::X19, Gpr::SP, 0),
            str_x_imm12(Gpr::X20, Gpr::SP, 8),
        ];
        let mut want = Vec::new();
        for w in expected_pro {
            want.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(prologue, want);

        let mut epilogue = Vec::new();
        frame.emit_epilogue(&mut epilogue);
        let expected_epi = [
            ldr_x_imm12(Gpr::X19, Gpr::SP, 0),
            ldr_x_imm12(Gpr::X20, Gpr::SP, 8),
            add_imm(Gpr::SP, Gpr::SP, 16),
            ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
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
