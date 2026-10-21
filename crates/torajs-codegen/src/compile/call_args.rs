//! AAPCS64 argument setup for a call site: stack lanes, then the
//! X0..X7 / V0..V7 register lanes as one parallel copy.

use torajs_core::ssa::{Operand, Type};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr, operand_is_f64};
use super::{FP_SCRATCH_LHS, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_TMP, write_u32};
use crate::enc::{fcvtzs_x_d, mov_d_reg, mov_x_reg, scvtf_d_x};
use crate::linear_scan_lanes::{ArgLane, classify_lanes};
use crate::reg::Reg;
use crate::regalloc::Assignment;

/// Whether argument `i` travels as an f64. The declared param type
/// decides when the callee has one at that position; varargs /
/// over-supplied args fall back to the operand's own classification.
fn expected_f64(i: usize, arg: &Operand, alloc: &Assignment, param_types: Option<&[Type]>) -> bool {
    match param_types {
        Some(pt) if i < pt.len() => pt[i] == Type::F64,
        _ => operand_is_f64(arg, alloc),
    }
}

fn arg_lanes(args: &[Operand], alloc: &Assignment, param_types: Option<&[Type]>) -> Vec<ArgLane> {
    let fp = args.iter().enumerate();
    classify_lanes(fp.map(|(i, arg)| expected_f64(i, arg, alloc, param_types)))
}

/// Does some argument travel on the stack?
pub(super) fn has_stack_lane(
    args: &[Operand],
    alloc: &Assignment,
    param_types: Option<&[Type]>,
) -> bool {
    arg_lanes(args, alloc, param_types)
        .iter()
        .any(|l| matches!(l, ArgLane::Stack(_)))
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
pub(super) fn pass_args(
    bytes: &mut Vec<u8>,
    args: &[Operand],
    alloc: &Assignment,
    param_types: Option<&[Type]>,
) {
    let lanes = arg_lanes(args, alloc, param_types);
    // Stack lanes first: their materializes read arbitrary homes
    // (which may include an ARG_RET register a later reg-lane move
    // would overwrite), while the stores themselves only touch the
    // outgoing area at [sp, #j*8] — carved into this frame's bottom by
    // `allocate_linear_scan`, so sp does not move.
    for (i, (arg, lane)) in args.iter().zip(&lanes).enumerate() {
        let ArgLane::Stack(j) = lane else { continue };
        let actual_is_f64 = operand_is_f64(arg, alloc);
        if expected_f64(i, arg, alloc, param_types) {
            let src = if actual_is_f64 {
                materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc)
            } else {
                let g = materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, scvtf_d_x(FP_SCRATCH_LHS, g));
                FP_SCRATCH_LHS
            };
            super::slot::sp_str_d(bytes, src, j * 8);
        } else {
            let src = if actual_is_f64 {
                let f = materialize_operand_fpr(bytes, arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, fcvtzs_x_d(OP_SCRATCH_LHS, f));
                OP_SCRATCH_LHS
            } else {
                materialize_operand_gpr(bytes, arg, OP_SCRATCH_LHS, alloc)
            };
            super::slot::sp_str_x(bytes, src, j * 8);
        }
    }
    // Register lanes are one parallel copy: an argument may still sit
    // in another argument's destination register.
    let mut moves: Vec<LaneMove> = args
        .iter()
        .zip(&lanes)
        .filter_map(|(arg, lane)| match lane {
            ArgLane::Reg(dst) => Some(LaneMove {
                dst: *dst,
                src: register_home(arg, alloc),
                arg,
            }),
            ArgLane::Stack(_) => None,
        })
        .collect();
    while !moves.is_empty() {
        let ready = (0..moves.len()).find(|&i| {
            let dst = Some(moves[i].dst);
            !moves
                .iter()
                .enumerate()
                .any(|(j, m)| j != i && m.src == dst)
        });
        match ready {
            Some(i) => {
                let m = moves.remove(i);
                emit_lane_move(bytes, &m, alloc);
            }
            None => {
                // every destination is still read: a cycle. Park one
                // destination's current value and redirect its readers
                let parked = moves[0].dst;
                let scratch = match parked {
                    Reg::Gpr(_) => Reg::Gpr(OP_SCRATCH_TMP),
                    _ => Reg::Fpr(FP_SCRATCH_RHS),
                };
                debug_assert!(moves.iter().all(|m| m.src != Some(scratch)));
                emit_reg_copy(bytes, scratch, parked);
                for m in moves.iter_mut().filter(|m| m.src == Some(parked)) {
                    m.src = Some(scratch);
                }
            }
        }
    }
}

/// One register-lane argument: `arg` goes to `dst`, read from `src`
/// when it lives in a register.
struct LaneMove<'a> {
    dst: Reg,
    src: Option<Reg>,
    arg: &'a Operand,
}

fn register_home(arg: &Operand, alloc: &Assignment) -> Option<Reg> {
    match arg {
        Operand::Value(v) if alloc.contains(*v) => match alloc.of(*v) {
            r @ (Reg::Gpr(_) | Reg::Fpr(_)) => Some(r),
            Reg::SpillGpr(_) | Reg::SpillFpr(_) => None,
        },
        _ => None,
    }
}

fn emit_reg_copy(bytes: &mut Vec<u8>, dst: Reg, src: Reg) {
    match (dst, src) {
        (Reg::Gpr(d), Reg::Gpr(s)) => write_u32(bytes, mov_x_reg(d, s)),
        (Reg::Fpr(d), Reg::Fpr(s)) => write_u32(bytes, mov_d_reg(d, s)),
        other => unreachable!("register copy across classes: {other:?}"),
    }
}

/// Place one argument. A class mismatch between the value and its
/// lane converts: FCVTZS (f64 into an i64-shaped param, JS ToInteger
/// truncation) or SCVTF (i64 into an f64 param).
fn emit_lane_move(bytes: &mut Vec<u8>, m: &LaneMove, alloc: &Assignment) {
    match (m.dst, m.src) {
        (Reg::Gpr(d), Some(Reg::Gpr(s))) if d != s => write_u32(bytes, mov_x_reg(d, s)),
        (Reg::Fpr(d), Some(Reg::Fpr(s))) if d != s => write_u32(bytes, mov_d_reg(d, s)),
        (Reg::Gpr(_), Some(Reg::Gpr(_))) | (Reg::Fpr(_), Some(Reg::Fpr(_))) => {}
        (Reg::Gpr(d), Some(Reg::Fpr(s))) => write_u32(bytes, fcvtzs_x_d(d, s)),
        (Reg::Fpr(d), Some(Reg::Gpr(s))) => write_u32(bytes, scvtf_d_x(d, s)),
        (Reg::Gpr(d), None) => {
            if operand_is_f64(m.arg, alloc) {
                let f =
                    materialize_operand_fpr(bytes, m.arg, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, fcvtzs_x_d(d, f));
            } else {
                let g = materialize_operand_gpr(bytes, m.arg, d, alloc);
                if g != d {
                    write_u32(bytes, mov_x_reg(d, g));
                }
            }
        }
        (Reg::Fpr(d), None) => {
            if operand_is_f64(m.arg, alloc) {
                let f = materialize_operand_fpr(bytes, m.arg, d, OP_SCRATCH_LHS, alloc);
                if f != d {
                    write_u32(bytes, mov_d_reg(d, f));
                }
            } else {
                let g = materialize_operand_gpr(bytes, m.arg, OP_SCRATCH_LHS, alloc);
                write_u32(bytes, scvtf_d_x(d, g));
            }
        }
        other => unreachable!("classify_lanes returned a non-arg register: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use torajs_core::ssa::id_hash::HashMap;

    use super::pass_args;
    use crate::enc::mov_x_reg;
    use crate::reg::{Gpr, Reg};
    use crate::regalloc::Assignment;
    use torajs_core::ssa::{Operand, ValueId};

    const GPRS: [Gpr; 16] = [
        Gpr::X0,
        Gpr::X1,
        Gpr::X2,
        Gpr::X3,
        Gpr::X4,
        Gpr::X5,
        Gpr::X6,
        Gpr::X7,
        Gpr::X8,
        Gpr::X9,
        Gpr::X10,
        Gpr::X11,
        Gpr::X12,
        Gpr::X13,
        Gpr::X14,
        Gpr::X15,
    ];

    /// Run `pass_args` for GPR values homed at `homes[i]`, passed in
    /// that order, then execute the emitted MOVs on a register file
    /// where each home initially holds its value's id. Returns the
    /// value found in X0..X(n-1).
    fn run(homes: &[Gpr]) -> Vec<u64> {
        let by_value: HashMap<u32, Reg> = homes
            .iter()
            .enumerate()
            .map(|(i, &g)| (i as u32, Reg::Gpr(g)))
            .collect();
        let alloc = Assignment::from_parts(by_value, HashMap::default(), 0, 0, true);
        let args: Vec<Operand> = (0..homes.len())
            .map(|i| Operand::Value(ValueId(i as u32)))
            .collect();
        let mut bytes = Vec::new();
        pass_args(&mut bytes, &args, &alloc, None);
        let mut regs = [u64::MAX; 16];
        for (i, &g) in homes.iter().enumerate() {
            regs[g.idx() as usize] = i as u64;
        }
        for w in bytes.chunks(4) {
            let w = u32::from_le_bytes(w.try_into().unwrap());
            let (d, s) = GPRS
                .iter()
                .flat_map(|&d| GPRS.iter().map(move |&s| (d, s)))
                .find(|&(d, s)| mov_x_reg(d, s) == w)
                .unwrap_or_else(|| panic!("not a register move: {w:08x}"));
            regs[d.idx() as usize] = regs[s.idx() as usize];
        }
        regs[..homes.len()].to_vec()
    }

    #[test]
    fn swapped_arguments_are_exchanged_through_a_scratch() {
        assert_eq!(run(&[Gpr::X1, Gpr::X0]), vec![0, 1]);
    }

    #[test]
    fn three_cycle_rotates() {
        assert_eq!(run(&[Gpr::X1, Gpr::X2, Gpr::X0]), vec![0, 1, 2]);
    }

    #[test]
    fn chain_is_written_back_to_front() {
        // x0 <- x1 <- x2 <- x13: overwriting x0 first would lose nothing,
        // overwriting x1 first would lose the value x0 wants
        assert_eq!(run(&[Gpr::X1, Gpr::X2, Gpr::X13]), vec![0, 1, 2]);
    }

    #[test]
    fn one_value_feeds_two_lanes_it_also_occupies() {
        let by_value: HashMap<u32, Reg> =
            HashMap::from_iter([(0, Reg::Gpr(Gpr::X1)), (1, Reg::Gpr(Gpr::X0))]);
        let alloc = Assignment::from_parts(by_value, HashMap::default(), 0, 0, true);
        let v = |i| Operand::Value(ValueId(i));
        let mut bytes = Vec::new();
        // f(v0, v0, v1): x0 <- x1, x1 <- x1, x2 <- x0
        pass_args(&mut bytes, &[v(0), v(0), v(1)], &alloc, None);
        let mut regs = [u64::MAX; 16];
        regs[1] = 0;
        regs[0] = 1;
        for w in bytes.chunks(4) {
            let w = u32::from_le_bytes(w.try_into().unwrap());
            let (d, s) = GPRS
                .iter()
                .flat_map(|&d| GPRS.iter().map(move |&s| (d, s)))
                .find(|&(d, s)| mov_x_reg(d, s) == w)
                .unwrap();
            regs[d.idx() as usize] = regs[s.idx() as usize];
        }
        assert_eq!(&regs[..3], &[0, 0, 1]);
    }
}
