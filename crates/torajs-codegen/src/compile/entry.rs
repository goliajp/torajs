//! Function entry: the prologue, then the call-crossing params moved
//! out of their arg registers and the stack params copied into their
//! homes.

use torajs_core::ssa::Function;

use super::pin_reg::PinRole;
use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS, slot, write_u32};
use crate::enc::{mov_d_reg, mov_x_reg};
use crate::frame::FrameLayout;
use crate::reg::Reg;
use crate::regalloc::Assignment;
use crate::shrink_wrap::ShrinkWrap;

/// Copy each stack param (9th+ argument of a register class) from the
/// caller's outgoing area into the home the allocator picked. The
/// incoming slots sit just above this frame: past the SP carve
/// (`frame_size`) and the FP/LR pair the prologue pushed — or at
/// `sp + j*8` directly when the frame is trivial (no prologue at
/// all). This is the only place the body ever touches the caller's
/// frame; everything downstream reads the copied home.
fn emit_stack_param_loads(bytes: &mut Vec<u8>, alloc: &Assignment, frame: &FrameLayout) {
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

/// Open the function's frame on entry: the prologue, then the
/// parameters moved to their homes. With a shrink-wrap save point,
/// returned, the prologue opens that block instead, and the blocks
/// ahead of it run frameless (see shrink_wrap.rs).
pub(super) fn open_frame(
    bytes: &mut Vec<u8>,
    func: &Function,
    alloc: &Assignment,
    frame: &FrameLayout,
    pin: PinRole,
) -> Option<ShrinkWrap> {
    // an entry loads x27 / x28 only once the prologue has saved them
    let shrink = if frame.is_trivial() || pin.is_entry() {
        None
    } else {
        crate::shrink_wrap::plan(func, alloc)
    };
    if shrink.is_none() {
        frame.emit_prologue(bytes);
        pin.emit_entry_loads(bytes);
        // Relocate call-crossing params out of their (caller-saved) arg
        // registers into the callee-saved registers the allocator picked,
        // now that the prologue has saved those registers' prior contents.
        emit_param_entry_moves(bytes, alloc);
        emit_stack_param_loads(bytes, alloc, frame);
    }
    shrink
}
