//! Block-terminator emit + the branch-fixup back-patching pass.
//!
//! Terminators are the one emit family that can't finish in a single
//! forward pass: a branch to a block that hasn't been laid out yet
//! has no displacement to encode. Each branch site therefore records
//! a [`BranchFixup`] with a placeholder word, and
//! [`patch_branch`] rewrites the displacement once every block's byte
//! offset is known.
//!
//! Layout-aware shaping lives here too — fall-through elision (drop a
//! `B` whose target is where execution lands anyway) and the CondBr
//! predicate flip that puts the hot successor on the not-taken path.

use std::collections::HashMap;

use torajs_core::ssa::{BlockId, Terminator};

use super::operand::{self, materialize_operand_fpr, materialize_operand_gpr};
use super::{OP_SCRATCH_LHS, brfuse, write_u32};
use crate::enc::{b_imm26, brk_imm16, cbnz_x, cbz_x, fmov_d_to_d, mov_x_reg, ret};
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
use crate::regalloc::Assignment;

#[derive(Debug, Clone, Copy)]
pub(super) enum BranchKind {
    /// B / BL — 26-bit signed displacement at bits 25-0.
    Imm26,
    /// CBZ / CBNZ / B.cond — 19-bit signed displacement at bits 23-5.
    Imm19,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BranchFixup {
    pub(super) site_byte_offset: u32,
    pub(super) target_block: BlockId,
    pub(super) kind: BranchKind,
}

pub(super) fn patch_branch(bytes: &mut [u8], fixup: &BranchFixup, displacement: i32) {
    debug_assert!(
        displacement % 4 == 0,
        "displacement {displacement} must be 4-byte aligned"
    );
    let inst_offset = displacement / 4;
    let offset = fixup.site_byte_offset as usize;
    let original = u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]);
    let patched = match fixup.kind {
        BranchKind::Imm26 => {
            let imm26 = (inst_offset as u32) & 0x03FF_FFFF;
            (original & !0x03FF_FFFF) | imm26
        }
        BranchKind::Imm19 => {
            let imm19 = (inst_offset as u32) & 0x7_FFFF;
            (original & !(0x7_FFFF << 5)) | (imm19 << 5)
        }
    };
    bytes[offset..offset + 4].copy_from_slice(&patched.to_le_bytes());
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_terminator(
    bytes: &mut Vec<u8>,
    fixups: &mut Vec<BranchFixup>,
    term: &Terminator,
    frame: &FrameLayout,
    alloc: &Assignment,
    fused: Option<&brfuse::FusedCmp>,
    blocks_by_id: &HashMap<u32, &torajs_core::ssa::Block>,
    next_resolved: Option<BlockId>,
) {
    let resolve = |t: BlockId| brfuse::resolve_forwarded_target(blocks_by_id, t);
    match term {
        Terminator::Ret(ret_op) => {
            // AAPCS64: int/ptr/bool return lands in X0, f64 in D0. The
            // SSA-emit's prior insts left the ret value in its allocated
            // home reg (X0 in trivial cases, or another GPR / spill slot
            // under linear scan); materialize it into the ABI slot before
            // the epilogue tears down the frame. Idempotent: if the
            // allocator already placed it in X0/V0, `materialize_operand_*`
            // returns the same reg and the mov is skipped. None ret_op
            // = `Stmt::Return;` or void-fn fall-through — leave X0/V0 alone
            // (void calls don't read the ret slot).
            //
            // Pre-fix observed silent-wrong: `function g(){return 5};
            // console.log(g())` emitted a single `ret` for g's body (no
            // const materialization), so X0 held _main's pre-call scratch
            // and console.log printed 1 instead of 5. The SSA Ret operand
            // was reaching emit_terminator but being discarded here.
            if let Some(op) = ret_op {
                if operand::operand_is_f64(op, alloc) {
                    let src = materialize_operand_fpr(bytes, op, Fpr::V0, OP_SCRATCH_LHS, alloc);
                    if src != Fpr::V0 {
                        write_u32(bytes, fmov_d_to_d(Fpr::V0, src));
                    }
                } else {
                    let src = materialize_operand_gpr(bytes, op, Gpr::X0, alloc);
                    if src != Gpr::X0 {
                        write_u32(bytes, mov_x_reg(Gpr::X0, src));
                    }
                }
            }
            frame.emit_epilogue(bytes);
            write_u32(bytes, ret(Gpr::X30));
        }
        Terminator::Br(target_block) => {
            // Fall-through elision: a B whose (forwarder-resolved)
            // target is where execution lands anyway is dead weight —
            // the popcount/collatz loop bodies string 4-5 of these
            // per iteration through pass-split blocks.
            if Some(resolve(*target_block)) == next_resolved {
                return;
            }
            let site = bytes.len() as u32;
            fixups.push(BranchFixup {
                site_byte_offset: site,
                target_block: *target_block,
                kind: BranchKind::Imm26,
            });
            // Emit B with placeholder displacement; patched later.
            write_u32(bytes, b_imm26(0));
        }
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } => {
            let else_falls = Some(resolve(*else_blk)) == next_resolved;
            // Layout-driven flip: when the then-target is the fall-
            // through block, branch on the inverted predicate to the
            // else-target instead — the hot successor (loop bodies
            // sit right after their header) runs on the not-taken
            // path and the trailing B disappears.
            let then_falls = !else_falls && Some(resolve(*then_blk)) == next_resolved;
            if let Some(fc) = fused {
                let (invert, br_target) = if then_falls {
                    (true, *else_blk)
                } else {
                    (false, *then_blk)
                };
                let trailing = (!then_falls && !else_falls).then_some(*else_blk);
                brfuse::emit_fused_condbr(bytes, fixups, fc, invert, br_target, trailing, alloc);
                return;
            }
            // Materialize the Bool cond into a GPR scratch. If the
            // cond is a Value, the allocator already placed it in a
            // register and materialize_operand_gpr is a fast lookup.
            let cond_reg = operand::materialize_operand_gpr(bytes, cond, OP_SCRATCH_LHS, alloc);
            // CBNZ cond, then_blk — branch when cond != 0 (i.e. true);
            // flipped to CBZ cond, else_blk when then falls through.
            let cb_site = bytes.len() as u32;
            fixups.push(BranchFixup {
                site_byte_offset: cb_site,
                target_block: if then_falls { *else_blk } else { *then_blk },
                kind: BranchKind::Imm19,
            });
            write_u32(
                bytes,
                if then_falls {
                    cbz_x(cond_reg, 0)
                } else {
                    cbnz_x(cond_reg, 0)
                },
            );
            if !then_falls && !else_falls {
                let b_site = bytes.len() as u32;
                fixups.push(BranchFixup {
                    site_byte_offset: b_site,
                    target_block: *else_blk,
                    kind: BranchKind::Imm26,
                });
                write_u32(bytes, b_imm26(0));
            }
        }
        Terminator::Unreachable => {
            // Clang/LLVM convention for `unreachable`.
            write_u32(bytes, brk_imm16(0xFD));
        }
    }
}
