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

use crate::enc::{add_imm, ldp_post_index, stp_pre_index, sub_imm};
use crate::reg::Gpr;

/// Layout descriptor for a function's stack frame.
#[derive(Debug, Clone, Copy)]
pub struct FrameLayout {
    /// Total alloca region bytes, 16-byte aligned per AAPCS64
    /// sp-alignment requirement.
    pub alloca_bytes: u32,
    /// True if the function makes any calls — set in S4 when Call /
    /// CallIndirect inst handling lands. Currently always false.
    pub uses_calls: bool,
}

impl FrameLayout {
    /// Trivial layout — no alloca, no calls. S1-S2 default.
    pub const fn leaf_no_spill() -> Self {
        Self {
            alloca_bytes: 0,
            uses_calls: false,
        }
    }

    /// Build a frame layout from raw alloca byte count. Rounds up to
    /// the AAPCS64 16-byte sp-alignment.
    pub fn from_alloca_bytes(raw_alloca_bytes: u32, uses_calls: bool) -> Self {
        let aligned = (raw_alloca_bytes + 15) & !15;
        Self {
            alloca_bytes: aligned,
            uses_calls,
        }
    }

    /// `true` when no prologue/epilogue code is needed.
    pub const fn is_trivial(&self) -> bool {
        self.alloca_bytes == 0 && !self.uses_calls
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
        // SUB sp, sp, #alloca_bytes  — carve alloca region
        if self.alloca_bytes > 0 {
            assert!(
                self.alloca_bytes < 4096,
                "S3-A2 frame size {} must fit in ADD/SUB imm12; larger frames land in S3-B with shifted-imm or scratch-reg path",
                self.alloca_bytes
            );
            write_word(bytes, sub_imm(Gpr::SP, Gpr::SP, self.alloca_bytes as u16));
        }
    }

    /// Emit the epilogue into `bytes`. No-op when trivial. RET is
    /// emitted separately by `compile::emit_terminator` immediately
    /// after this.
    pub fn emit_epilogue(&self, bytes: &mut Vec<u8>) {
        if self.is_trivial() {
            return;
        }
        // ADD sp, sp, #alloca_bytes  — release alloca region
        if self.alloca_bytes > 0 {
            write_word(bytes, add_imm(Gpr::SP, Gpr::SP, self.alloca_bytes as u16));
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
}
