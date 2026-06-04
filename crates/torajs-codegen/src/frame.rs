//! AAPCS64 stack frame protocol.
//!
//! S1 is leaf-function-only: no calls, no spills, no callee-save
//! regs touched, no FP/LR storage needed. For such fns the prologue
//! / epilogue are empty — the body runs in caller's frame.
//!
//! Full prologue/epilogue layout (S3+ when alloca / spills land):
//!
//! ```text
//!   high addr
//!   ┌───────────────────────────┐  ← caller sp
//!   │  saved x29 (frame ptr)    │
//!   │  saved x30 (link reg)     │
//!   ├───────────────────────────┤  ← new x29 (frame ptr) ↓
//!   │  callee-saved regs        │
//!   │  alloca region            │
//!   │  spill slots              │
//!   └───────────────────────────┘  ← new sp = x29 - frame_size
//!   low addr
//! ```
//!
//! Prologue (when frame_size > 0):
//!   STP x29, x30, [sp, #-16]!     # save FP+LR, sp -= 16
//!   MOV x29, sp                   # FP = sp
//!   SUB sp, sp, #frame_size       # carve out alloca + spill
//!
//! Epilogue:
//!   ADD sp, sp, #frame_size       # release alloca + spill
//!   LDP x29, x30, [sp], #16       # restore FP+LR, sp += 16
//!   RET                           # branch x30
//!
//! S1 implementation: when `frame_size == 0` (no alloca, no spill,
//! no calls), emit nothing — the leaf function just dispatches.

/// Layout descriptor for a function's stack frame. S1 keeps it as
/// just `frame_size`; S3+ adds callee-save register set + alloca
/// region offsets.
#[derive(Debug, Clone, Copy)]
pub struct FrameLayout {
    /// Total bytes carved out below the saved FP/LR pair. Must be a
    /// 16-byte multiple per AAPCS64 sp-alignment requirement.
    pub frame_size: u32,
    /// Whether the function makes any calls (and therefore needs to
    /// save x29/x30). S1: always `false`. S4+: set when the SSA has
    /// any `Call` / `CallIndirect` inst.
    pub uses_calls: bool,
}

impl FrameLayout {
    /// Empty layout — leaf, no alloca / spill / calls. S1 default.
    pub const fn leaf_no_spill() -> Self {
        Self {
            frame_size: 0,
            uses_calls: false,
        }
    }

    /// `true` when no prologue/epilogue code is needed.
    pub const fn is_trivial(&self) -> bool {
        self.frame_size == 0 && !self.uses_calls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_no_spill_is_trivial() {
        let f = FrameLayout::leaf_no_spill();
        assert!(f.is_trivial());
        assert_eq!(f.frame_size, 0);
        assert!(!f.uses_calls);
    }

    #[test]
    fn nonzero_frame_is_not_trivial() {
        let f = FrameLayout {
            frame_size: 16,
            uses_calls: false,
        };
        assert!(!f.is_trivial());
    }

    #[test]
    fn uses_calls_is_not_trivial_even_with_zero_frame() {
        let f = FrameLayout {
            frame_size: 0,
            uses_calls: true,
        };
        assert!(!f.is_trivial());
    }
}
