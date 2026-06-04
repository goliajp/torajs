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

use crate::enc::{b_imm26, brk_imm16, cbnz_x, ret};
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
use crate::regalloc::{Assignment, allocate_trivial};
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
/// FPR scratches for FP-side operand materialization.
pub(crate) const FP_SCRATCH_LHS: Fpr = Fpr::V16;
pub(crate) const FP_SCRATCH_RHS: Fpr = Fpr::V17;

/// Output of `compile_function` — raw aarch64 bytes + per-function
/// reloc table, ready to hand off to torajs-obj (#7).
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub bytes: Vec<u8>,
    pub relocs: Vec<Reloc>,
    pub frame: FrameLayout,
}

/// Compile one SSA Function to aarch64 bytes.
pub fn compile_function(func: &Function) -> CompiledFunction {
    let alloc = allocate_trivial(func);
    let frame = FrameLayout::from_alloca_bytes(alloc.raw_alloca_bytes, alloc.has_calls);
    let mut bytes: Vec<u8> = Vec::new();
    let mut relocs: Vec<Reloc> = Vec::new();
    let mut fixups: Vec<BranchFixup> = Vec::new();

    // Track each block's start byte offset so the branch-fixup pass
    // can compute PC-relative displacements after all blocks emit.
    let mut block_byte_starts: HashMap<u32, u32> = HashMap::with_capacity(func.blocks.len());

    frame.emit_prologue(&mut bytes);

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
        InstKind::BinOp(op, lhs, rhs) => emit_binop(bytes, inst, op, lhs, rhs, alloc),
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
        Terminator::Ret(_) => {
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
}
