//! Function entry, right after the prologue: move call-crossing
//! params out of their arg registers and copy stack params into
//! their homes.

use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS, slot, write_u32};
use crate::enc::{mov_d_reg, mov_x_reg};
use crate::frame::FrameLayout;
use crate::reg::Reg;
use crate::regalloc::Assignment;

/// Copy each stack param (9th+ argument of a register class) from the
/// caller's outgoing area into the home the allocator picked. The
/// incoming slots sit just above this frame: past the SP carve
/// (`frame_size`) and the FP/LR pair the prologue pushed — or at
/// `sp + j*8` directly when the frame is trivial (no prologue at
/// all). This is the only place the body ever touches the caller's
/// frame; everything downstream reads the copied home.
pub(super) fn emit_stack_param_loads(bytes: &mut Vec<u8>, alloc: &Assignment, frame: &FrameLayout) {
    if alloc.stack_param_entry_loads.is_empty() {
        return;
    }
    let base = if frame.is_trivial() {
        0
    } else {
        frame.frame_size() + 16
    };
    for &(slot, dst) in &alloc.stack_param_entry_loads {
        let off = base + slot * 8;
        match dst {
            Reg::Gpr(d) => slot::sp_ldr_x(bytes, d, off),
            Reg::Fpr(d) => slot::sp_ldr_d(bytes, d, off),
            Reg::SpillGpr(soff) => {
                slot::sp_ldr_x(bytes, OP_SCRATCH_LHS, off);
                slot::sp_str_x(bytes, OP_SCRATCH_LHS, soff);
            }
            Reg::SpillFpr(soff) => {
                slot::sp_ldr_d(bytes, FP_SCRATCH_LHS, off);
                slot::sp_str_d(bytes, FP_SCRATCH_LHS, soff);
            }
        }
    }
}

/// Emit the param entry-moves the allocator recorded: each call-
/// crossing param is relocated from its caller-saved AAPCS64 arg
/// register into the callee-saved register (or spill slot) it was
/// assigned, immediately after the prologue saved those registers.
pub(super) fn emit_param_entry_moves(bytes: &mut Vec<u8>, alloc: &Assignment) {
    for &(src, dst) in &alloc.param_entry_moves {
        match (src, dst) {
            (Reg::Gpr(s), Reg::Gpr(d)) => write_u32(bytes, mov_x_reg(d, s)),
            (Reg::Fpr(s), Reg::Fpr(d)) => write_u32(bytes, mov_d_reg(d, s)),
            (Reg::Gpr(s), Reg::SpillGpr(off)) => slot::sp_str_x(bytes, s, off),
            (Reg::Fpr(s), Reg::SpillFpr(off)) => slot::sp_str_d(bytes, s, off),
            other => unreachable!("param entry move with mismatched reg classes: {other:?}"),
        }
    }
}
