//! Function-level compile driver.
//!
//! Walks `torajs_core::ssa::Function` once, dispatches each `InstKind`
//! to the matching aarch64 instruction sequence via `enc::*`, and
//! emits the little-endian byte stream + any `Reloc` descriptors.
//!
//! Split into sub-modules per InstKind family to keep each file (and
//! its inline test block) under the 500-LOC hard limit:
//!
//!   - [`operand`]: `materialize_operand_{gpr,fpr}` +
//!     `materialize_const_i64` (MOVZ+MOVK quadrant chain). All
//!     constant-to-register lowering lives here.
//!   - [`binop`]: `emit_binop` for the full integer + float BinOp
//!     surface (`BinOp::Add..LShr` + `FAdd..FDiv`).
//!   - [`cmp`]: `emit_icmp` / `emit_fcmp` lowering ICmp/FCmp to
//!     CMP/FCMP + CSET with `ipred_to_cond` / `fpred_to_cond` mapping
//!     tables.
//!   - [`cast`]: `emit_bitcast_{f64_to_i64,i64_to_f64}` — single FMOV
//!     between Xn and Dn, no bit conversion.
//!
//! S1 baseline test (`fn one_plus_two() -> i64 { 1 + 2 }`) lives here
//! in `mod tests` because it's a driver-level acceptance test, not
//! tied to any one InstKind family.
//!
//! Phase-level coverage notes (history, kept for reference):
//!
//! S1 — Add only. S2-A — full integer BinOp + 64-bit ConstI64 via
//! MOVZ+MOVK. S2-B1 — FP encoders + Fpr enum. S2-B2 — FP binop wire-
//! up + BitCast + Operand::ConstF64 via FMOV-d-from-x. S2-C — ICmp +
//! FCmp via CMP/FCMP + CSET. S2-D pending: FRem (libm `fmod` call)
//! and FPred::One (CCMP fold or NE+VC+AND).

mod binop;
mod call;
mod cast;
mod cmp;
mod mem;
mod operand;
mod refs;

#[cfg(test)]
pub(crate) mod test_fixtures;

use std::collections::HashMap;

use torajs_core::ssa::{BlockId, Function, InstKind, Terminator};

use crate::enc::{
    b_imm26, brk_imm16, cbnz_x, fmov_d_to_d, mov_x_reg, ret, str_d_imm12, str_x_imm12,
};
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::Reloc;

pub use binop::emit_binop;
pub use call::{emit_call, emit_call_indirect};
pub use cast::{
    emit_bitcast_f64_to_i64, emit_bitcast_i64_to_f64, emit_fp_to_si, emit_int_to_ptr,
    emit_ptr_to_int, emit_si_to_fp, emit_trunc_i64_to_bool, emit_zext_bool_to_i64,
    emit_zext_i32_to_i64,
};
pub use cmp::{emit_fcmp, emit_icmp};
pub use mem::{emit_alloca, emit_load, emit_load_dyn, emit_store, emit_store_dyn};
pub use operand::{materialize_const_i64, materialize_operand_fpr, materialize_operand_gpr};
pub use refs::{emit_fn_addr, emit_global_ref, emit_static_str_ref, emit_string_ref};

/// Operand scratch GPRs — sub-modules use these to materialize int
/// constants (`materialize_const_i64`) and arrange operands.
pub(crate) const OP_SCRATCH_LHS: Gpr = Gpr::X9;
pub(crate) const OP_SCRATCH_RHS: Gpr = Gpr::X10;
/// Tertiary GPR scratch for compound ops (e.g. SREM = SDIV+MSUB).
pub(crate) const OP_SCRATCH_TMP: Gpr = Gpr::X11;
/// LS-3 spill-result scratch (GPR). When `Assignment::def_gpr`
/// returns `(scratch, Some(off))`, the emitter writes its result
/// into `OP_SCRATCH_RESULT_GPR` and then `STR scratch, [SP, #off]`.
/// Reserved out of `CALLER_SAVED_SCRATCH` (X12) so the allocator
/// never hands it to a live SSA value.
pub(crate) const OP_SCRATCH_RESULT_GPR: Gpr = Gpr::X12;
/// FPR scratches for FP-side operand materialization.
pub(crate) const FP_SCRATCH_LHS: Fpr = Fpr::V16;
pub(crate) const FP_SCRATCH_RHS: Fpr = Fpr::V17;
/// LS-3 spill-result scratch (FPR). Mirror of `OP_SCRATCH_RESULT_GPR`
/// for f64 defs. Reserved out of `FP_CALLER_SAVED_SCRATCH` (V18).
pub(crate) const FP_SCRATCH_RESULT: Fpr = Fpr::V18;

/// Output of `compile_function` — raw aarch64 bytes + per-function
/// reloc table, ready to hand off to torajs-obj (#7).
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub bytes: Vec<u8>,
    pub relocs: Vec<Reloc>,
    pub frame: FrameLayout,
}

/// Compile one SSA Function to aarch64 bytes — convenience wrapper
/// over [`compile_function_with`] that uses the Linear Scan
/// allocator (LS-4 cut-over from trivial).
pub fn compile_function(func: &Function) -> CompiledFunction {
    compile_function_with(func, crate::linear_scan::allocate_linear_scan(func))
}

/// Compile one SSA Function to aarch64 bytes using a pre-computed
/// register assignment. Lets callers swap allocators without going
/// through the default `allocate_trivial` path; LS-4 will route
/// `compile_function` through `allocate_linear_scan` here once the
/// spill-aware emitters land in S5-gap-2-3 (this commit).
pub fn compile_function_with(func: &Function, alloc: Assignment) -> CompiledFunction {
    // Frame carves alloca + spill + callee-save area in one sp
    // adjustment. `used_callee_*_mask` is 0 for leaf / non-crossing
    // functions, so this matches the pre-callee-save layout
    // byte-for-byte; call-crossing functions get the callee-save area
    // above the spill region (offsets unchanged — see `FrameLayout`).
    let frame = FrameLayout::with_callee_saved(
        alloc.total_frame_bytes(),
        alloc.used_callee_gpr_mask,
        alloc.used_callee_fpr_mask,
        alloc.has_calls,
    );
    let mut bytes: Vec<u8> = Vec::new();
    let mut relocs: Vec<Reloc> = Vec::new();
    let mut fixups: Vec<BranchFixup> = Vec::new();

    // Track each block's start byte offset so the branch-fixup pass
    // can compute PC-relative displacements after all blocks emit.
    let mut block_byte_starts: HashMap<u32, u32> = HashMap::with_capacity(func.blocks.len());

    frame.emit_prologue(&mut bytes);
    // Relocate call-crossing params out of their (caller-saved) arg
    // registers into the callee-saved registers the allocator picked,
    // now that the prologue has saved those registers' prior contents.
    emit_param_entry_moves(&mut bytes, &alloc);

    for block in &func.blocks {
        block_byte_starts.insert(block.id.0, bytes.len() as u32);
        for inst in &block.insts {
            emit_inst(&mut bytes, &mut relocs, inst, &alloc);
        }
        emit_terminator(&mut bytes, &mut fixups, &block.term, &frame, &alloc);
    }

    // Patch every branch fixup with its real displacement.
    for fixup in &fixups {
        let target_byte_offset =
            *block_byte_starts
                .get(&fixup.target_block.0)
                .unwrap_or_else(|| {
                    panic!(
                        "Branch target BlockId({}) not in function",
                        fixup.target_block.0
                    )
                });
        let displacement = target_byte_offset as i32 - fixup.site_byte_offset as i32;
        patch_branch(&mut bytes, fixup, displacement);
    }

    CompiledFunction {
        name: func.name.clone(),
        bytes,
        relocs,
        frame,
    }
}

#[derive(Debug, Clone, Copy)]
enum BranchKind {
    /// B / BL — 26-bit signed displacement at bits 25-0.
    Imm26,
    /// CBZ / CBNZ / B.cond — 19-bit signed displacement at bits 23-5.
    Imm19,
}

#[derive(Debug, Clone, Copy)]
struct BranchFixup {
    site_byte_offset: u32,
    target_block: BlockId,
    kind: BranchKind,
}

fn patch_branch(bytes: &mut [u8], fixup: &BranchFixup, displacement: i32) {
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

fn emit_inst(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &torajs_core::ssa::Inst,
    alloc: &Assignment,
) {
    match &inst.kind {
        InstKind::BinOp(op, lhs, rhs) => emit_binop(bytes, relocs, inst, op, lhs, rhs, alloc),
        InstKind::ICmp(pred, lhs, rhs) => emit_icmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::FCmp(pred, lhs, rhs) => emit_fcmp(bytes, inst, *pred, lhs, rhs, alloc),
        InstKind::BitCastF64ToI64(src) => emit_bitcast_f64_to_i64(bytes, inst, src, alloc),
        InstKind::BitCastI64ToF64(src) => emit_bitcast_i64_to_f64(bytes, inst, src, alloc),
        InstKind::SiToFp(src) => emit_si_to_fp(bytes, inst, src, alloc),
        InstKind::FpToSi(src) => emit_fp_to_si(bytes, inst, src, alloc),
        InstKind::ZExtBoolToI64(src) => emit_zext_bool_to_i64(bytes, inst, src, alloc),
        InstKind::ZExtI32ToI64(src) => emit_zext_i32_to_i64(bytes, inst, src, alloc),
        InstKind::TruncI64ToBool(src) => emit_trunc_i64_to_bool(bytes, inst, src, alloc),
        InstKind::PtrToInt(src) => emit_ptr_to_int(bytes, inst, src, alloc),
        InstKind::IntToPtr(src) => emit_int_to_ptr(bytes, inst, src, alloc),
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => emit_alloca(bytes, inst, alloc),
        InstKind::Load(ty, ptr, offset) => emit_load(bytes, inst, ty, ptr, *offset, alloc),
        InstKind::Store(val, ptr, offset) => emit_store(bytes, val, ptr, *offset, alloc),
        InstKind::LoadDyn(ty, base, dyn_offset) => {
            emit_load_dyn(bytes, inst, ty, base, dyn_offset, alloc)
        }
        InstKind::StoreDyn(val, base, dyn_offset) => {
            emit_store_dyn(bytes, val, base, dyn_offset, alloc)
        }
        InstKind::Call(func_id, args) => emit_call(bytes, relocs, inst, *func_id, args, alloc),
        InstKind::CallIndirect(sig_id, fn_ptr, args) => {
            emit_call_indirect(bytes, inst, *sig_id, fn_ptr, args, alloc)
        }
        InstKind::GlobalRef(name) => emit_global_ref(bytes, relocs, inst, name, alloc),
        InstKind::StringRef(string_id) => emit_string_ref(bytes, relocs, inst, *string_id, alloc),
        InstKind::StaticStrRef(string_id) => {
            emit_static_str_ref(bytes, relocs, inst, *string_id, alloc)
        }
        InstKind::FnAddr(func_id) => emit_fn_addr(bytes, relocs, inst, *func_id, alloc),
    }
}

fn emit_terminator(
    bytes: &mut Vec<u8>,
    fixups: &mut Vec<BranchFixup>,
    term: &Terminator,
    frame: &FrameLayout,
    alloc: &Assignment,
) {
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
            // Materialize the Bool cond into a GPR scratch. If the
            // cond is a Value, the allocator already placed it in a
            // register and materialize_operand_gpr is a fast lookup.
            let cond_reg = operand::materialize_operand_gpr(bytes, cond, OP_SCRATCH_LHS, alloc);
            // CBNZ cond, then_blk — branch when cond != 0 (i.e. true).
            let cbnz_site = bytes.len() as u32;
            fixups.push(BranchFixup {
                site_byte_offset: cbnz_site,
                target_block: *then_blk,
                kind: BranchKind::Imm19,
            });
            write_u32(bytes, cbnz_x(cond_reg, 0));
            // Fall-through to else_blk via unconditional B.
            let b_site = bytes.len() as u32;
            fixups.push(BranchFixup {
                site_byte_offset: b_site,
                target_block: *else_blk,
                kind: BranchKind::Imm26,
            });
            write_u32(bytes, b_imm26(0));
        }
        Terminator::Unreachable => {
            // Clang/LLVM convention for `unreachable`.
            write_u32(bytes, brk_imm16(0xFD));
        }
    }
}

pub(crate) fn write_u32(bytes: &mut Vec<u8>, word: u32) {
    bytes.extend_from_slice(&word.to_le_bytes());
}

/// LS-3 def-side spill: if `spill_off` is `Some(off)`, emit
/// `STR scratch, [SP, #off]` — the result of the just-emitted
/// instruction was written into `scratch` because the SSA value's
/// allocator slot is a `Reg::SpillGpr`. No-op when the value lives
/// in a real GPR (`spill_off == None`).
pub(crate) fn write_def_spill_gpr(bytes: &mut Vec<u8>, spill_off: Option<u32>, scratch: Gpr) {
    if let Some(off) = spill_off {
        write_u32(bytes, crate::enc::str_x_imm12(scratch, Gpr::SP, off));
    }
}

/// FPR-class mirror of [`write_def_spill_gpr`].
pub(crate) fn write_def_spill_fpr(bytes: &mut Vec<u8>, spill_off: Option<u32>, scratch: Fpr) {
    if let Some(off) = spill_off {
        write_u32(bytes, crate::enc::str_d_imm12(scratch, Gpr::SP, off));
    }
}

/// Emit the param entry-moves the allocator recorded: each call-
/// crossing param is relocated from its caller-saved AAPCS64 arg
/// register into the callee-saved register (or spill slot) it was
/// assigned, immediately after the prologue saved those registers.
fn emit_param_entry_moves(bytes: &mut Vec<u8>, alloc: &Assignment) {
    for &(src, dst) in &alloc.param_entry_moves {
        match (src, dst) {
            (Reg::Gpr(s), Reg::Gpr(d)) => write_u32(bytes, mov_x_reg(d, s)),
            (Reg::Fpr(s), Reg::Fpr(d)) => write_u32(bytes, fmov_d_to_d(d, s)),
            (Reg::Gpr(s), Reg::SpillGpr(off)) => write_u32(bytes, str_x_imm12(s, Gpr::SP, off)),
            (Reg::Fpr(s), Reg::SpillFpr(off)) => write_u32(bytes, str_d_imm12(s, Gpr::SP, off)),
            other => unreachable!("param entry move with mismatched reg classes: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::{build_one_plus_two, words_to_le_bytes};

    /// S1 acceptance — `fn one_plus_two() -> i64 { 1 + 2 }` shape
    /// compiles end-to-end to the hand-encoded 16-byte reference:
    ///
    /// ```text
    ///   MOVZ x9, #1       0xD2800029
    ///   MOVZ x10, #2      0xD280004A
    ///   ADD x0, x9, x10   0x8B0A0120
    ///   RET               0xD65F03C0
    /// ```
    #[test]
    fn one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[0xD280_0029, 0xD280_004A, 0x8B0A_0120, 0xD65F_03C0]);
        assert_eq!(
            compiled.bytes, expected,
            "byte stream mismatch — expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.name, "one_plus_two");
        assert!(compiled.relocs.is_empty(), "S1 has no relocs");
        assert!(
            compiled.frame.is_trivial(),
            "leaf fn must have trivial frame"
        );
    }

    #[test]
    fn one_plus_two_emits_exactly_16_bytes() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);
        assert_eq!(compiled.bytes.len(), 16, "4 instructions × 4 bytes");
    }

    // --- S5: terminator + multi-block branch layout ---

    /// `fn unreachable_fn() { unreachable }` — single block, single
    /// BRK #0xFD instruction. Trivial frame.
    #[test]
    fn unreachable_terminator_emits_brk() {
        use torajs_core::ssa::{Block, BlockId, Function, Type};
        let func = Function {
            name: "unreachable_fn".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: Vec::new(),
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Unreachable,
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[crate::enc::brk_imm16(0xFD)]);
        assert_eq!(compiled.bytes, expected);
    }

    /// `fn br_to_block1() -> i64 { block0: B block1; block1: ret 1 }` —
    /// two blocks, B in block 0 forward-branches to block 1.
    #[test]
    fn br_forward_to_next_block_byte_equal() {
        use torajs_core::ssa::{
            Block, BlockId, Function, Inst, InstKind, Operand, Type, ValueId, ValueInfo,
        };

        let v0 = ValueId(0);
        let func = Function {
            name: "br_to_block1".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: Vec::new(),
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::ZExtBoolToI64(Operand::ConstBool(true)),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(v0))),
                },
            ],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        // block 0 @ 0:  B  +4  (target = block 1 @ 4)
        // block 1 @ 4:  MOVZ x9, #1
        //               AND  x0, x9, #1
        //               RET
        let expected = words_to_le_bytes(&[
            crate::enc::b_imm26(4),
            crate::enc::movz_imm(Gpr::X9, 1, 0),
            crate::enc::and_imm_one(Gpr::X0, Gpr::X9),
            crate::enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "br_to_block1: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    /// `fn cond_branch() -> i64 { CondBr(true, then=block1, else=block2) ... }` —
    /// 3-block fixture verifying CondBr's CBNZ+B emit pair and the
    /// 2-fixup patching pass.
    #[test]
    fn cond_branch_three_block_byte_equal() {
        use torajs_core::ssa::{
            Block, BlockId, Function, Inst, InstKind, Operand, Type, ValueId, ValueInfo,
        };

        let v1 = ValueId(0);
        let v2 = ValueId(1);
        let func = Function {
            name: "cond_branch".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v2".into()),
                },
            ],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: Vec::new(),
                    term: Terminator::CondBr {
                        cond: Operand::ConstBool(true),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(v1),
                        kind: InstKind::ZExtBoolToI64(Operand::ConstBool(true)),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(v1))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![Inst {
                        result: Some(v2),
                        kind: InstKind::ZExtBoolToI64(Operand::ConstBool(false)),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(v2))),
                },
            ],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        // block 0 @ 0:  MOVZ x9, #1            (materialize cond=true)
        //               CBNZ x9, +8            (then=block1 @ 12)
        //               B    +16               (else=block2 @ 24)
        // block 1 @ 12: MOVZ x9, #1
        //               AND  x0, x9, #1
        //               RET
        // block 2 @ 24: MOVZ x9, #0
        //               AND  x0, x9, #1
        //               RET
        let expected = words_to_le_bytes(&[
            crate::enc::movz_imm(Gpr::X9, 1, 0),
            crate::enc::cbnz_x(Gpr::X9, 8),
            crate::enc::b_imm26(16),
            crate::enc::movz_imm(Gpr::X9, 1, 0),
            crate::enc::and_imm_one(Gpr::X0, Gpr::X9),
            crate::enc::ret(Gpr::X30),
            crate::enc::movz_imm(Gpr::X9, 0, 0),
            crate::enc::and_imm_one(Gpr::X0, Gpr::X9),
            crate::enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "cond_branch: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    // ---- LS-3 end-to-end spill integration ----

    /// Compile a 12-live-values function under the Linear Scan
    /// allocator (NOT trivial — trivial would panic on pool
    /// exhaustion). The emitted byte stream must:
    ///   - prologue carves a frame ≥ 16 bytes for spill slots,
    ///   - contain at least one STR + at least one LDR against SP
    ///     (the spilled value's def write-back + its later use load),
    ///   - epilogue releases the same frame size,
    ///   - end with RET.
    /// This is the headline acceptance test for LS-3: it proves the
    /// def-side `def_gpr` + write_def_spill_gpr machinery and the
    /// operand-side LDR-from-spill machinery wire up correctly when
    /// the allocator actually issues a SpillGpr.
    #[test]
    fn ls_spill_end_to_end_byte_stream_has_str_and_ldr() {
        use crate::enc;
        use crate::linear_scan::allocate_linear_scan;
        use torajs_core::ssa::{
            BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type,
            ValueId, ValueInfo,
        };

        let f0 = FuncId(0);
        let mut values = Vec::new();
        let mut insts = Vec::new();
        for i in 0..12 {
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("v{i}")),
            });
            insts.push(Inst {
                result: Some(ValueId(i as u32)),
                kind: InstKind::Call(f0, Vec::new()),
                origin: None,
            });
        }
        let mut acc = Operand::Value(ValueId(0));
        for i in 1..12 {
            let nv = ValueId(12 + i as u32 - 1);
            values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("a{i}")),
            });
            insts.push(Inst {
                result: Some(nv),
                kind: InstKind::BinOp(BinOp::Add, acc, Operand::Value(ValueId(i as u32))),
                origin: None,
            });
            acc = Operand::Value(nv);
        }
        let func = Function {
            name: "twelve_live_spill".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(acc)),
            }],
            current_origin: None,
        };
        let alloc = allocate_linear_scan(&func);
        assert!(
            alloc.total_spill_bytes > 0,
            "fixture must overflow pool — got {} spill bytes",
            alloc.total_spill_bytes
        );

        let compiled = compile_function_with(&func, alloc);

        // Frame carves 16-rounded (alloca + spill) and saves FP/LR.
        assert!(
            !compiled.frame.is_trivial(),
            "spill must produce non-trivial frame"
        );
        assert!(compiled.frame.uses_calls);
        assert!(
            compiled.frame.alloca_bytes >= 16,
            "spill region must be ≥ 16 bytes (one 16-aligned slot pair)"
        );

        // Decode the byte stream, count STR/LDR against SP (imm12
        // variant). Each spilled SSA value contributes at least one
        // STR (def write-back) and one LDR (final-use read). For
        // OP_SCRATCH_RESULT_GPR (X12) writes/reads we expect Rn=SP.
        let mut str_to_sp = 0;
        let mut ldr_from_sp = 0;
        for chunk in compiled.bytes.chunks_exact(4) {
            let w = u32::from_le_bytes(chunk.try_into().unwrap());
            if (w & 0xFFC0_03E0) == (enc::str_x_imm12(Gpr::X12, Gpr::SP, 0) & 0xFFC0_03E0)
                && ((w >> 5) & 0x1F) == 31
            {
                str_to_sp += 1;
            }
            if (w & 0xFFC0_03E0) == (enc::ldr_x_imm12(Gpr::X9, Gpr::SP, 0) & 0xFFC0_03E0)
                && ((w >> 5) & 0x1F) == 31
            {
                ldr_from_sp += 1;
            }
        }
        // Each STR also pattern-matches LDR (different opc bit) so
        // the masks need to be distinct. Use the raw base words:
        // STR Xn, [SP, #imm] base = 0xF900_0000 with Rn=31 → bits
        // 31..22 = 1111100100, Rn @ bits 9..5 = 11111. LDR same with
        // bit 22 set: base = 0xF940_0000.
        let mut strs = 0;
        let mut ldrs = 0;
        for chunk in compiled.bytes.chunks_exact(4) {
            let w = u32::from_le_bytes(chunk.try_into().unwrap());
            let rn = (w >> 5) & 0x1F;
            if rn != 31 {
                continue;
            }
            if (w & 0xFFC0_0000) == 0xF900_0000 {
                strs += 1;
            }
            if (w & 0xFFC0_0000) == 0xF940_0000 {
                ldrs += 1;
            }
        }
        // Sanity: the brittle approximate counts above can stay as
        // debug aids but the substantive assertion uses the precise
        // base-word match.
        let _ = (str_to_sp, ldr_from_sp);
        assert!(strs >= 1, "spilled defs must emit STR [SP, #off]");
        assert!(ldrs >= 1, "spilled uses must emit LDR [SP, #off]");

        // Last word is RET.
        let last_word = u32::from_le_bytes(
            compiled.bytes[compiled.bytes.len() - 4..]
                .try_into()
                .unwrap(),
        );
        assert_eq!(last_word, enc::ret(Gpr::X30), "function must end with RET");
    }

    /// Pre-cut-over invariant: while `compile_function` still uses
    /// `allocate_trivial` (LS-4 will swap), the byte stream for
    /// existing fixtures must be byte-identical to pre-LS-3 (the
    /// scratch-pool shift X12→X13 / V16→V19 was the only LS-3
    /// behavior-visible change; spill paths are dormant under
    /// trivial). The 147 prior unit tests covering binop / cmp /
    /// cast / mem / call / refs all pass without modification.
    /// This test asserts the simplest of those (one_plus_two) one
    /// more time to make the no-regression invariant explicit.
    #[test]
    fn ls3_does_not_regress_trivial_byte_stream() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[0xD280_0029, 0xD280_004A, 0x8B0A_0120, 0xD65F_03C0]);
        assert_eq!(compiled.bytes, expected);
    }
}
