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

mod arr_push;
mod binop;
mod bittest;
mod box_any;
mod brfuse;
mod call;
mod call_args;
mod call_carry;
mod cast;
mod cell;
mod cmp;
mod cond_flags;
mod ctpop_range_sum;
mod entry;
mod fusions;
mod global_access;
mod in_module;
mod inline_sites;
mod inst;
mod landing;
mod low32;
mod mem;
mod operand;
pub(crate) mod pin_reg;
mod rc;
mod rc_dec;
mod refs;
mod selinc;
mod sibcall;
pub(crate) mod slot;
mod store_pair;
mod str_append;
mod substr_drop;
mod terminator;
mod throw_probe;
mod throw_reg;
mod throw_reg_emit;
mod throw_reg_lax;
mod throw_reg_zero;
mod vec;

#[cfg(test)]
mod pin_reg_tests;
#[cfg(test)]
pub(crate) mod test_fixtures;
#[cfg(test)]
mod throw_reg_tests;

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{Block, Function};

use throw_reg::Role;

use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
use crate::regalloc::Assignment;
use crate::reloc::Reloc;
use crate::shrink_wrap::ShrinkWrap;
use entry::{emit_param_entry_moves, emit_stack_param_loads};
use inst::emit_inst_at;
use terminator::{BranchFixup, BranchKind, Fixups, emit_terminator};

pub use binop::{emit_binop, emit_ctpop, emit_cttz, emit_neg};
pub use call::{emit_call, emit_call_indirect};
pub use cast::{
    emit_bitcast_f64_to_i64, emit_bitcast_i64_to_f64, emit_fp_to_int32, emit_fp_to_si,
    emit_int_to_ptr, emit_ptr_to_int, emit_si_to_fp, emit_trunc_i64_to_bool, emit_zext_bool_to_i64,
    emit_zext_i32_to_i64,
};
pub use cell::CellOps;
pub use cmp::{emit_fcmp, emit_icmp};
pub use ctpop_range_sum::emit_ctpop_range_sum;
pub use in_module::{ModuleFacts, compile_function_at};
pub use inline_sites::InlineSites;
pub use mem::{
    emit_alloca, emit_load, emit_load_dyn, emit_load_dyn_scaled8, emit_load_u8_dyn,
    emit_load_u16_dyn, emit_store, emit_store_dyn, emit_store_dyn_scaled8,
};
pub use operand::{
    emit_copy, materialize_const_i64, materialize_operand_fpr, materialize_operand_gpr,
};
pub use refs::{
    emit_boxed_entry_addr, emit_fn_addr, emit_global_ref, emit_static_str_ref, emit_string_ref,
};
pub use str_append::StrAppendLit;

/// Code offset a non-fall-through block is padded to. Function
/// starts are 64-aligned by the linker, so this is absolute.
const BRANCH_TARGET_ALIGN: usize = 16;

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
///
/// Test convenience: callers without a module's cross-fn sig table
/// pass nothing. Production builds must use
/// [`compile_function_in`] so call sites can coerce f64 args
/// flowing into i64 params (substr / parseInt-with-fractional-radix
/// etc.); without that table, mismatched-type Calls silently send
/// the raw f64 bits into the int reg lane.
pub fn compile_function(func: &Function) -> CompiledFunction {
    compile_function_in(func, &ModuleFacts::default())
}

/// [`compile_function`] with the module's facts, for a function
/// outside the module's call graph (no throw register role).
pub fn compile_function_in(func: &Function, facts: &ModuleFacts) -> CompiledFunction {
    compile_function_with(
        func,
        crate::linear_scan::allocate_linear_scan_with(func, facts.inline),
        facts,
    )
}

/// Compile one SSA Function to aarch64 bytes using a pre-computed
/// register assignment.
pub fn compile_function_with(
    func: &Function,
    alloc: Assignment,
    facts: &ModuleFacts,
) -> CompiledFunction {
    in_module::compile_with_role(func, alloc, facts, Default::default())
}

/// The function's code and relocations; None when a branch landed out
/// of its reach (a bit test, or a conditional branch `far` left short).
fn emit_body(
    func: &Function,
    alloc: &Assignment,
    facts: &ModuleFacts,
    frame: &FrameLayout,
    (role, pin): (Role, pin_reg::PinRole),
    (bit_tests, far): (bool, bool),
) -> Option<(Vec<u8>, Vec<Reloc>)> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut relocs: Vec<Reloc> = Vec::new();
    let mut fixups = Fixups::new(far);

    // each block's start offset, for the branch-fixup pass's displacements
    let mut block_byte_starts: HashMap<u32, u32> =
        HashMap::with_capacity_and_hasher(func.blocks.len(), Default::default());

    let shrink = open_frame(&mut bytes, func, alloc, frame, pin);
    let frameless = FrameLayout::leaf_no_spill();

    let fusions = fusions::Fusions::of(func, alloc, facts, role, bit_tests);

    // Where each branch lands, shared by the terminator emit (fall-
    // through elision needs the next block's final identity) and the
    // branch-fixup pass below.
    let landing = landing::Landing::of(func, alloc, &shrink);

    // A block no predecessor falls into starts on a 16-byte fetch
    // boundary, unless it is cold. The pad sits after an unconditional
    // branch or a return, so it never runs.
    let mut falls_in = true;
    // slot index of the block's first inst, numbered as in liveness
    let mut block_slot: u32 = 0;
    let emitted = landing.emitted(&func.blocks);
    let mut throws = throw_reg_emit::BlockHooks::of(func, facts, role, frame, &landing);
    for (bi, block) in func.blocks.iter().enumerate() {
        if !emitted[bi] {
            block_slot += block.insts.len() as u32 + 1;
            continue;
        }
        if !falls_in && !alloc.is_cold(bi) {
            while !bytes.len().is_multiple_of(BRANCH_TARGET_ALIGN) {
                write_u32(&mut bytes, crate::enc::nop());
            }
        }
        let start = bytes.len();
        block_byte_starts.insert(block.id.0, start as u32);
        let (alloc, frame) =
            landing::open_block(&mut bytes, block, &shrink, alloc, frame, &frameless);
        if throws.open(&mut bytes, &mut fixups, block, frame, start) {
            block_slot += block.insts.len() as u32 + 1;
            falls_in = false;
            continue;
        }
        let probe = fusions.reads_throw_reg(block.id.0);
        let fused = fusions.cmp_of(block.id.0).filter(|_| !probe);
        let sib = sibcall::plan(func, block, &landing.blocks_by_id, alloc, facts, role)
            .filter(|_| !pin.is_entry());
        let body = sib.as_ref().map_or(block.insts.len(), |s| s.at);
        let at = (block, body, block_slot);
        emit_block_insts(
            &mut bytes,
            &mut relocs,
            func,
            at,
            alloc,
            facts,
            &fusions,
            &throws,
        );
        block_slot += block.insts.len() as u32 + 1;
        if let Some(s) = &sib {
            sibcall::emit(&mut bytes, &mut relocs, s, alloc, facts, frame);
            falls_in = false;
            continue;
        }
        // The layout successor's final identity — a branch resolving
        // there is a fall-through and needs no B (the next emitted
        // block, or the empty-forwarder chain it heads, is about to
        // emit).
        let next = terminator::Next {
            here: block.id,
            resolved: (bi + 1..func.blocks.len())
                .find(|&k| emitted[k])
                .map(|k| landing.resolve(func.blocks[k].id)),
        };
        assert!(!landing.is_hollow(block.id) || bytes.len() == start);
        emit_terminator(
            &mut bytes,
            &mut fixups,
            &block.term,
            frame,
            alloc,
            fused,
            &landing,
            next,
            terminator::ThrowExit {
                probe,
                signal: role.signals && throw_reg::has_signal(block),
            },
        );
        falls_in = terminator::falls_through(&block.term, &landing, next);
    }

    let patched = terminator::patch_all(&mut bytes, &fixups, &landing, &block_byte_starts);
    patched.then_some((bytes, relocs))
}

/// Open the function's frame on entry: the prologue, then the
/// parameters moved to their homes. With a shrink-wrap save point,
/// returned, the prologue opens that block instead, and the blocks
/// ahead of it run frameless (see shrink_wrap.rs).
fn open_frame(
    bytes: &mut Vec<u8>,
    func: &Function,
    alloc: &Assignment,
    frame: &FrameLayout,
    pin: pin_reg::PinRole,
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

/// The instructions of `block` ahead of `end` (a sibling call, or the
/// terminator); `slot` numbers the first as liveness does.
#[allow(clippy::too_many_arguments)] // the block loop's state, passed through
fn emit_block_insts(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    func: &Function,
    (block, end, slot): (&Block, usize, u32),
    alloc: &Assignment,
    facts: &ModuleFacts,
    fusions: &fusions::Fusions,
    throws: &throw_reg_emit::BlockHooks,
) {
    let mut flags = cond_flags::CondFlags::default();
    // never across a block start: another way in may not have run
    // the call before
    let mut carry = None;
    for (ii, inst) in block.insts[..end].iter().enumerate() {
        if !(fusions.emit(bytes, relocs, block, ii, inst, alloc)
            || flags.emit_select(bytes, inst, alloc))
        {
            let site = slot + ii as u32;
            emit_inst_at(bytes, relocs, func, inst, alloc, facts, site, &mut carry);
        }
        throws.after_inst(bytes, block, ii);
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
        slot::sp_str_x(bytes, scratch, off);
    }
}

/// FPR-class mirror of [`write_def_spill_gpr`].
pub(crate) fn write_def_spill_fpr(bytes: &mut Vec<u8>, spill_off: Option<u32>, scratch: Fpr) {
    if let Some(off) = spill_off {
        slot::sp_str_d(bytes, scratch, off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::{build_one_plus_two, words_to_le_bytes};

    /// S1 acceptance — `fn one_plus_two() -> i64 { 1 + 2 }` shape
    /// compiles end-to-end to the hand-encoded 12-byte reference
    /// (Round 5 popcount branch/const attack: the const RHS takes
    /// the ADD-imm12 form instead of a MOVZ + register ADD):
    ///
    /// ```text
    ///   MOVZ x9, #1       0xD2800029
    ///   ADD x0, x9, #2    0x91000920
    ///   RET               0xD65F03C0
    /// ```
    #[test]
    fn one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[0xD280_0029, 0x9100_0920, 0xD65F_03C0]);
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
    fn one_plus_two_emits_exactly_12_bytes() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);
        assert_eq!(compiled.bytes.len(), 12, "3 instructions × 4 bytes");
    }

    // --- S5: terminator + multi-block branch layout ---

    /// `fn unreachable_fn() { unreachable }` — single block, single
    /// BRK #0xFD instruction. Trivial frame.
    #[test]
    fn unreachable_terminator_emits_brk() {
        use torajs_core::ssa::{Block, BlockId, Function, Terminator, Type};
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
            Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
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

        // block 0 @ 0:  (B elided — block 1 is the fall-through)
        // block 1 @ 0:  MOVZ x9, #1
        //               AND  x0, x9, #1
        //               RET
        let expected = words_to_le_bytes(&[
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
            Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
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
        //               CBZ  x9, +28           (layout flip: then is
        //                                       the fall-through, so
        //                                       branch the inverse to
        //                                       else=block2 @ 32)
        // block 1 @ 8:  MOVZ x9, #1
        //               AND  x0, x9, #1
        //               RET
        //         @ 20: NOP ×3                 (block 2 follows a RET:
        //                                       pad to a 16-byte boundary)
        // block 2 @ 32: MOVZ x9, #0
        //               AND  x0, x9, #1
        //               RET
        let expected = words_to_le_bytes(&[
            crate::enc::movz_imm(Gpr::X9, 1, 0),
            crate::enc::cbz_x(Gpr::X9, 28),
            crate::enc::movz_imm(Gpr::X9, 1, 0),
            crate::enc::and_imm_one(Gpr::X0, Gpr::X9),
            crate::enc::ret(Gpr::X30),
            crate::enc::nop(),
            crate::enc::nop(),
            crate::enc::nop(),
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
    ///
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

        let compiled = compile_function_with(&func, alloc, &ModuleFacts::default());

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
        for chunk in compiled.bytes.as_chunks::<4>().0 {
            let w = u32::from_le_bytes(*chunk);
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
        for chunk in compiled.bytes.as_chunks::<4>().0 {
            let w = u32::from_le_bytes(*chunk);
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
}
