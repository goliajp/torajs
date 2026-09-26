//! AAPCS64 argument setup for a call site: stack lanes, then the
//! X0..X7 / V0..V7 register lanes.

use torajs_core::ssa::{Operand, Type};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr, operand_is_f64};
use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS, write_u32};
use crate::enc::{fcvtzs_x_d, mov_d_reg, mov_x_reg, scvtf_d_x, str_d_imm12, str_x_imm12};
use crate::linear_scan_lanes::{ArgLane, classify_lanes};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;

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
pub(super) fn pass_args(
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
                        write_u32(bytes, mov_d_reg(*arg_reg, src));
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
