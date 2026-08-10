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

use torajs_core::ssa::{FuncId, Inst, Operand, SigId, Type};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr, operand_is_f64};
use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS, write_u32};
use crate::enc::{
    bl_imm26, blr_reg, fcvtzs_x_d, fmov_d_to_d, mov_x_reg, scvtf_d_x, str_d_imm12, str_x_imm12,
};
use crate::linear_scan_lanes::{ArgLane, classify_lanes};
use crate::reg::{Fpr, Gpr, Reg};
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
    param_types: Option<&[Type]>,
) {
    pass_args(bytes, args, alloc, param_types);

    // Record reloc for the BL site, then emit BL with displacement=0.
    let bl_byte_offset = bytes.len() as u32;
    relocs.push(Reloc {
        byte_offset: bl_byte_offset,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(target_func),
        },
    });
    write_u32(bytes, bl_imm26(0));

    // After BL, the AAPCS64 ret value is in X0 (int / ptr) or V0 (f64).
    // If the allocator placed the Call's result elsewhere, MOV/FMOV
    // it across — see `route_call_ret`.
    if let Some(result_vid) = inst.result {
        route_call_ret(bytes, alloc, result_vid);
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
    // Arg setup first so the fn_ptr scratch isn't overwritten by an
    // arg materialization. The fn_ptr is materialized last and
    // therefore lives in OP_SCRATCH_LHS or its assigned register.
    //
    // SigId-driven param-type coercion is not yet threaded for
    // CallIndirect (param_types: None below) — the surface S4-C
    // exercises (closure dispatch in typed form) hasn't surfaced an
    // f64-into-i64 mismatch yet. When it does, plumb `module.sigs[
    // sig_id.0]` here the same way `compile_function_with_sigs`
    // plumbs fn_sigs for direct Calls.
    pass_args(bytes, args, alloc, None);

    let fn_ptr_reg = materialize_operand_gpr(bytes, fn_ptr, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, blr_reg(fn_ptr_reg));

    if let Some(result_vid) = inst.result {
        route_call_ret(bytes, alloc, result_vid);
    }
}

/// AAPCS64 §5.4.2 arg passing — int / ptr args fill X0..X7 in their
/// own lane while f64 args fill V0..V7 (D0..D7) in a parallel lane.
/// The two counters advance independently, matching how clang lowers
/// `void foo(int, double, int, double)` to (X0, D0, X1, D1).
///
/// `param_types: Some(&[Type])` enables call-boundary coercion: when
/// the actual operand's reg class disagrees with the declared param
/// type, emit FCVTZS (f64→i64) or SCVTF (i64→f64) before placing the
/// value in its AAPCS64 lane. Mirrors the LLVM-backend behaviour in
/// `ssa_inkwell::lower_inst::InstKind::Call`, which inserts
/// `build_float_to_signed_int` / `build_signed_int_to_float` against
/// `callee.get_type().get_param_types()` — same matrix, here written
/// at the byte level.
///
/// Surfaces this matters for in real code:
///   - `'abc'.substr(-1.1)` lowers to `Call(__torajs_str_substr,
///     [strPtr, ConstF64(-1.1), ConstI64(MAX)])` — `__torajs_str_substr`
///     declares `(Str, I64, I64) -> Str`. Without coercion the f64 bits
///     land in V0 (FP lane) while the runtime reads X1, gives garbage.
///   - `parseInt("123", 2.5)` likewise feeds an f64 radix into an i64
///     param; coercion truncates the fractional part per JS ToInteger.
///   - The Bool/I32/Ptr lanes are 8-byte int-shape today so they fall
///     through to the same int lane without an extra coerce step;
///     widen this matrix when the corpus surfaces a real mismatch.
///
/// `param_types: None` keeps the legacy "trust the operand-side reg
/// class" behaviour for callers without a sig table (CallIndirect,
/// test fixtures via the empty-sigs `compile_function` wrapper).
fn pass_args(
    bytes: &mut Vec<u8>,
    args: &[Operand],
    alloc: &Assignment,
    param_types: Option<&[Type]>,
) {
    // Expected lane: prefer the declared param type when available
    // and within the declared arity; varargs / over-supplied args
    // fall back to the operand's own classification.
    let expected_f64 = |i: usize, arg: &Operand| match param_types {
        Some(pt) if i < pt.len() => pt[i] == Type::F64,
        _ => operand_is_f64(arg, alloc),
    };
    let lanes = classify_lanes(args.iter().enumerate().map(|(i, arg)| expected_f64(i, arg)));
    // Stack lanes first: their materializes read arbitrary homes
    // (which may include an ARG_RET register a later reg-lane move
    // would overwrite), while the stores themselves only touch the
    // outgoing area at [sp, #j*8] — carved into this frame's bottom by
    // `allocate_linear_scan`, so sp does not move.
    for (i, (arg, lane)) in args.iter().zip(&lanes).enumerate() {
        let ArgLane::Stack(j) = lane else { continue };
        let actual_is_f64 = operand_is_f64(arg, alloc);
        if expected_f64(i, arg) {
            let src = if actual_is_f64 {
                materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc)
            } else {
                let g = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, scvtf_d_x(FP_SCRATCH_LHS, g));
                FP_SCRATCH_LHS
            };
            write_u32(bytes, str_d_imm12(src, Gpr::SP, j * 8));
        } else {
            let src = if actual_is_f64 {
                let f = materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, fcvtzs_x_d(OP_SCRATCH_LHS, f));
                OP_SCRATCH_LHS
            } else {
                materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc)
            };
            write_u32(bytes, str_x_imm12(src, Gpr::SP, j * 8));
        }
    }
    for (arg, lane) in args.iter().zip(&lanes) {
        let ArgLane::Reg(reg) = lane else { continue };
        let actual_is_f64 = operand_is_f64(arg, alloc);
        match reg {
            Reg::Fpr(arg_reg) => {
                if actual_is_f64 {
                    let src =
                        materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                    if src != *arg_reg {
                        write_u32(bytes, fmov_d_to_d(*arg_reg, src));
                    }
                } else {
                    // i64-shaped operand into an f64 param: SCVTF the int
                    // value into the FP arg lane.
                    let src = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
                    write_u32(bytes, scvtf_d_x(*arg_reg, src));
                }
            }
            Reg::Gpr(arg_reg) => {
                if actual_is_f64 {
                    // f64 operand into an i64-shaped param: FCVTZS
                    // (truncate toward zero) from the FP src register
                    // straight into the int arg lane. Mirrors JS
                    // ToInteger semantics on call boundaries.
                    let src =
                        materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                    write_u32(bytes, fcvtzs_x_d(*arg_reg, src));
                } else {
                    let src = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
                    if src != *arg_reg {
                        write_u32(bytes, mov_x_reg(*arg_reg, src));
                    }
                }
            }
            other => unreachable!("classify_lanes returned a non-arg register: {other:?}"),
        }
    }
}

/// Route the post-call return value to wherever the allocator placed
/// the SSA result. Per AAPCS64 §6.4.1 int / ptr returns live in X0,
/// f64 returns in V0 (D0):
///
///   - `Reg::Gpr(X0)`: no-op.
///   - `Reg::Gpr(other)`: MOV other, X0.
///   - `Reg::SpillGpr(off)`: STR X0, [SP, #off].
///   - `Reg::Fpr(V0)`: no-op.
///   - `Reg::Fpr(other)`: FMOV other, V0 (D-form).
///   - `Reg::SpillFpr(off)`: STR D0, [SP, #off].
///
/// X0 / V0 are the AAPCS64 caller-saved ret slots so a direct
/// STR from them needs no scratch detour.
fn route_call_ret(bytes: &mut Vec<u8>, alloc: &Assignment, result_vid: torajs_core::ssa::ValueId) {
    match alloc.of(result_vid) {
        Reg::Gpr(Gpr::X0) => {}
        Reg::Gpr(dst) => write_u32(bytes, mov_x_reg(dst, Gpr::X0)),
        Reg::SpillGpr(off) => write_u32(bytes, str_x_imm12(Gpr::X0, Gpr::SP, off)),
        Reg::Fpr(Fpr::V0) => {}
        Reg::Fpr(dst) => write_u32(bytes, fmov_d_to_d(dst, Fpr::V0)),
        Reg::SpillFpr(off) => write_u32(bytes, str_d_imm12(Fpr::V0, Gpr::SP, off)),
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
    /// CallIndirect. Allocator gives v0 (GlobalRef result, Ptr) → X13
    /// scratch; v1 (CallIndirect result, I64 ret) → X0.
    ///
    /// Expected sequence:
    ///
    /// ```text
    ///   STP x29, x30, [SP, #-16]!
    ///   MOV x29, sp
    ///   ADRP x13, page(foo_ptr)     (Page21 reloc at 8)
    ///   ADD  x13, x13, #pageoff(.)  (PageOff12 reloc at 12)
    ///   BLR  x13                    (indirect call)
    ///   LDP x29, x30, [SP], #16
    ///   RET
    /// ```
    #[test]
    fn caller_calls_indirect_via_globalref_byte_equal() {
        use torajs_core::ssa::SigId;

        let v0 = ValueId(0); // GlobalRef("foo_ptr") result → X13
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
            enc::adrp(Gpr::X13, 0),
            enc::add_imm(Gpr::X13, Gpr::X13, 0),
            enc::blr_reg(Gpr::X13),
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
