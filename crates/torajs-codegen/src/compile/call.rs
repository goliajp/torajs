//! Call lowering — emit BL with a CallSite reloc and arrange args /
//! return value per AAPCS64.
//!
//! S4-A scope: `InstKind::Call(FuncId, args)` with int-shaped args
//! only (each loaded into x0..x7 by simple MOV reg-reg). The BL
//! instruction is emitted with byte_offset=0; torajs-link (#8) patches
//! the real displacement after symbol resolution.
//!
//! S4-C adds `InstKind::CallIndirect(SigId, fn_ptr, args)`: the fn
//! pointer Operand materializes into a GPR scratch (or comes from
//! an SSA Value's assigned register), and the call uses BLR Xn
//! instead of BL. No reloc is needed — the address is dynamic.
//!
//! Limitations still pending S4-D / S5+:
//!   - FP args (need v0..v7 via FMOV).
//!   - > 8 int args (stack-pass per AAPCS64 §5.4.2).
//!   - Caller-saved registers live across a call (need spill+reload).

use torajs_core::ssa::{FuncId, Inst, Operand, SigId};

use super::OP_SCRATCH_LHS;
use super::operand::materialize_operand_gpr;
use super::write_u32;
use crate::enc::{bl_imm26, blr_reg, mov_x_reg};
use crate::reg::{Gpr, aapcs64};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

/// Emit a direct call: arg setup + `BL #0` + post-call ret-register
/// move (if the allocator placed the result somewhere other than x0).
///
/// Records a `Reloc::CallSite { target_func }` so torajs-link can
/// fix up the 26-bit branch displacement after symbol resolution.
pub fn emit_call(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    target_func: FuncId,
    args: &[Operand],
    alloc: &Assignment,
) {
    // S4-A: arg passing — each int arg goes into x0..x7. FP args /
    // stack args lift to S4-B.
    assert!(
        args.len() <= aapcs64::ARG_RET.len(),
        "S4-A supports up to {} int args; got {} (stack-pass lands in S4-B)",
        aapcs64::ARG_RET.len(),
        args.len()
    );
    for (idx, arg) in args.iter().enumerate() {
        let arg_reg = aapcs64::ARG_RET[idx];
        let src = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
        if src != arg_reg {
            write_u32(bytes, mov_x_reg(arg_reg, src));
        }
    }

    // Record reloc for the BL site, then emit BL with displacement=0.
    let bl_byte_offset = bytes.len() as u32;
    relocs.push(Reloc {
        byte_offset: bl_byte_offset,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(target_func),
        },
    });
    write_u32(bytes, bl_imm26(0));

    // After BL, the AAPCS64 ret value is in x0. If the allocator placed
    // the Call's result elsewhere (e.g. trivial alloc gave it a scratch
    // because the Call isn't itself the function's ret), MOV it across.
    if let Some(result_vid) = inst.result {
        let dst = alloc.of(result_vid).as_gpr();
        if dst != Gpr::X0 {
            write_u32(bytes, mov_x_reg(dst, Gpr::X0));
        }
    }
}

/// Emit an indirect call: arg setup + `BLR Xn` + post-call ret-
/// register move. No reloc — the target address is in `Xn` at
/// runtime, not a link-time-resolved symbol.
///
/// `_sig_id` is reserved for the future ABI-by-signature dispatch
/// (e.g. fp args via v0..v7); S4-C only handles int args via x0..x7
/// like `emit_call`.
pub fn emit_call_indirect(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    _sig_id: SigId,
    fn_ptr: &Operand,
    args: &[Operand],
    alloc: &Assignment,
) {
    assert!(
        args.len() <= aapcs64::ARG_RET.len(),
        "S4-C supports up to {} int args; got {} (stack-pass lands in S4+)",
        aapcs64::ARG_RET.len(),
        args.len()
    );

    // Arg setup first so the fn_ptr scratch isn't overwritten by an
    // arg materialization. The fn_ptr is materialized last and
    // therefore lives in OP_SCRATCH_LHS or its assigned register.
    for (idx, arg) in args.iter().enumerate() {
        let arg_reg = aapcs64::ARG_RET[idx];
        let src = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
        if src != arg_reg {
            write_u32(bytes, mov_x_reg(arg_reg, src));
        }
    }

    let fn_ptr_reg = materialize_operand_gpr(bytes, fn_ptr, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, blr_reg(fn_ptr_reg));

    if let Some(result_vid) = inst.result {
        let dst = alloc.of(result_vid).as_gpr();
        if dst != Gpr::X0 {
            write_u32(bytes, mov_x_reg(dst, Gpr::X0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::words_to_le_bytes;
    use crate::enc;
    use crate::reg::Gpr;
    use crate::reloc::{CallTarget, RelocKind};
    use torajs_core::ssa::{
        Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    /// S4-A acceptance — minimal call shape:
    ///
    /// ```text
    /// fn caller() -> i64 {
    ///     foo()
    /// }
    /// ```
    ///
    /// Allocator gives v0 (Call result, also ret) → x0, so no post-
    /// call MOV is needed. FrameLayout sees `has_calls=true` with
    /// `alloca_bytes=0`, so the prologue is STP+MOV (no SUB) and the
    /// epilogue is LDP (no ADD).
    ///
    /// Expected 6-instruction sequence:
    ///
    /// ```text
    ///   prologue:
    ///     STP x29, x30, [SP, #-16]!
    ///     MOV x29, sp
    ///   body:
    ///     BL #0                    (with CallSite reloc at offset 8)
    ///   epilogue:
    ///     LDP x29, x30, [SP], #16
    ///     RET
    /// ```
    #[test]
    fn caller_calls_foo_byte_equal_and_records_reloc() {
        let v0 = ValueId(0);
        let foo_id = FuncId(42);
        let func = Function {
            name: "caller".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::Call(foo_id, Vec::new()),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[
            enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            enc::add_imm(Gpr::X29, Gpr::SP, 0),
            enc::bl_imm26(0),
            enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "caller_calls_foo: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 20, "5 inst × 4 bytes");

        // BL site lives immediately after the 2-inst prologue at byte
        // offset 8.
        assert_eq!(compiled.relocs.len(), 1, "exactly one CallSite reloc");
        let reloc = &compiled.relocs[0];
        assert_eq!(reloc.byte_offset, 8);
        match &reloc.kind {
            RelocKind::CallSite {
                target: CallTarget::Func(target_func),
            } => {
                assert_eq!(*target_func, foo_id);
            }
            other => panic!("expected CallSite/Func, got {other:?}"),
        }

        // Frame: uses_calls=true, alloca_bytes=0 → not trivial.
        assert!(!compiled.frame.is_trivial());
        assert_eq!(compiled.frame.alloca_bytes, 0);
        assert!(compiled.frame.uses_calls);
    }

    /// S4-C acceptance — `fn caller() -> i64 { (*foo_ptr)() }` via
    /// CallIndirect. Allocator gives v0 (GlobalRef result, Ptr) → X12
    /// scratch; v1 (CallIndirect result, I64 ret) → X0.
    ///
    /// Expected sequence:
    ///
    /// ```text
    ///   STP x29, x30, [SP, #-16]!
    ///   MOV x29, sp
    ///   ADRP x12, page(foo_ptr)     (Page21 reloc at 8)
    ///   ADD  x12, x12, #pageoff(.)  (PageOff12 reloc at 12)
    ///   BLR  x12                    (indirect call)
    ///   LDP x29, x30, [SP], #16
    ///   RET
    /// ```
    #[test]
    fn caller_calls_indirect_via_globalref_byte_equal() {
        use torajs_core::ssa::SigId;

        let v0 = ValueId(0); // GlobalRef("foo_ptr") result → X12
        let v1 = ValueId(1); // CallIndirect result, ret → X0
        let func = Function {
            name: "caller_indirect".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::Ptr,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::GlobalRef("foo_ptr".to_string()),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::CallIndirect(SigId(0), Operand::Value(v0), Vec::new()),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v1))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        let expected = words_to_le_bytes(&[
            enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            enc::add_imm(Gpr::X29, Gpr::SP, 0),
            enc::adrp(Gpr::X12, 0),
            enc::add_imm(Gpr::X12, Gpr::X12, 0),
            enc::blr_reg(Gpr::X12),
            enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "caller_indirect: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 28, "7 inst × 4 bytes");

        // Two relocs (Page21 + PageOff12) from the GlobalRef; the BLR
        // itself doesn't reloc.
        assert_eq!(compiled.relocs.len(), 2);
        match &compiled.relocs[0].kind {
            RelocKind::Page21 { target_sym } => assert_eq!(target_sym, "foo_ptr"),
            other => panic!("expected Page21, got {other:?}"),
        }
        match &compiled.relocs[1].kind {
            RelocKind::PageOff12 { target_sym } => assert_eq!(target_sym, "foo_ptr"),
            other => panic!("expected PageOff12, got {other:?}"),
        }

        // Frame: uses_calls (BLR sets x30 too).
        assert!(!compiled.frame.is_trivial());
        assert!(compiled.frame.uses_calls);
    }
}
