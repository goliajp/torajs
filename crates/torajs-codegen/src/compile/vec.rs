//! `Type::Vec` values: Q-register loads, stores, copies and the lane
//! arithmetic of `InstKind::Vec`.
//!
//! A vector value always has a register home (`vec_homes`): a spill
//! slot would hold half of it. One live across a call is stored whole
//! around the call.

use torajs_core::ssa::{Inst, Operand, ValueId, VecInst, VecOp, VecShape};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR,
    OP_SCRATCH_RHS, OP_SCRATCH_TMP, write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_imm, add_v2d, addp_d_v2d, and_v16b, bic_v16b, bif_v16b, bit_v16b, bsl_v16b, cmeq_v2d,
    cmge_v2d, cmgt_v2d, dup_2d_d, dup_2d_x, dup_d_d1, ext_v16b_8, fadd_v2d, fcmeq_v2d, fcmge_v2d,
    fcmgt_v2d, fcvtzs_v2d, fdiv_v2d, fmov_d_from_x, fmov_x_from_d, fmul_v2d, frintz_v2d, fsub_v2d,
    ins_d1_d0, ins_d1_x, ldr_q_imm12, ldur_q_imm9, mov_d_reg, movi_2d_ones, movi_2d_zero, orr_reg,
    orr_v16b, str_q_imm12, stur_q_imm9, sub_v2d, umov_x_d,
};
use crate::reg::{Fpr, Gpr, Reg};
use crate::regalloc::Assignment;

fn vec_home(alloc: &Assignment, v: ValueId) -> Fpr {
    match alloc.of(v) {
        Reg::Fpr(f) => f,
        other => panic!("vector ValueId({}) has no vector register: {other:?}", v.0),
    }
}

fn vec_operand(alloc: &Assignment, op: &Operand) -> Fpr {
    match op {
        Operand::Value(v) => vec_home(alloc, *v),
        other => panic!("vector operand must be a value, got {other:?}"),
    }
}

/// The base register and immediate a Q access at `base + offset`
/// encodes with; a far offset is added into the scratch first.
fn q_address(bytes: &mut Vec<u8>, ptr: &Operand, offset: u64, alloc: &Assignment) -> (Gpr, u64) {
    let rn = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_LHS, alloc);
    if offset.is_multiple_of(16) && offset / 16 < 4096 || offset < 256 {
        return (rn, offset);
    }
    assert!(offset < 4096, "vector access offset {offset} out of range");
    write_u32(bytes, add_imm(OP_SCRATCH_LHS, rn, offset as u16));
    (OP_SCRATCH_LHS, 0)
}

pub(super) fn emit_vec_load(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let dst = vec_home(alloc, inst.result.expect("vector load has a result"));
    let (rn, off) = q_address(bytes, ptr, offset, alloc);
    if off.is_multiple_of(16) && off / 16 < 4096 {
        write_u32(bytes, ldr_q_imm12(dst, rn, off as u32));
    } else {
        write_u32(bytes, ldur_q_imm9(dst, rn, off as i32));
    }
}

pub(super) fn emit_vec_store(
    bytes: &mut Vec<u8>,
    val: &Operand,
    ptr: &Operand,
    offset: u64,
    alloc: &Assignment,
) {
    let src = vec_operand(alloc, val);
    let (rn, off) = q_address(bytes, ptr, offset, alloc);
    if off.is_multiple_of(16) && off / 16 < 4096 {
        write_u32(bytes, str_q_imm12(src, rn, off as u32));
    } else {
        write_u32(bytes, stur_q_imm9(src, rn, off as i32));
    }
}

pub(super) fn emit_vec_copy(bytes: &mut Vec<u8>, inst: &Inst, op: &Operand, alloc: &Assignment) {
    let dst = vec_home(alloc, inst.result.expect("Copy has a result"));
    let src = vec_operand(alloc, op);
    if dst != src {
        write_u32(bytes, mov_d_reg(dst, src));
    }
}

pub(super) fn emit_vec(bytes: &mut Vec<u8>, inst: &Inst, vi: &VecInst, alloc: &Assignment) {
    let result = inst.result.expect("vector instruction has a result");
    match vi {
        VecInst::Splat(_, Operand::ConstI64(0)) => {
            write_u32(bytes, movi_2d_zero(vec_home(alloc, result)));
        }
        VecInst::Splat(VecShape::I64x2, Operand::ConstI64(-1)) => {
            write_u32(bytes, movi_2d_ones(vec_home(alloc, result)));
        }
        VecInst::Splat(VecShape::F64x2, scalar) => {
            let src = materialize_operand_fpr(bytes, scalar, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, dup_2d_d(vec_home(alloc, result), src));
        }
        VecInst::Splat(VecShape::I64x2, scalar) => {
            let src = materialize_operand_gpr(bytes, scalar, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, dup_2d_x(vec_home(alloc, result), src));
        }
        // the low lane is the f64 register itself; the high lane is
        // written after it, from a scratch when the destination is
        // the high operand's own register
        VecInst::Pair(VecShape::F64x2, lo, hi) => {
            let dst = vec_home(alloc, result);
            let lo = materialize_operand_fpr(bytes, lo, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            let mut hi = materialize_operand_fpr(bytes, hi, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
            if hi == dst {
                write_u32(bytes, mov_d_reg(FP_SCRATCH_RHS, hi));
                hi = FP_SCRATCH_RHS;
            }
            if dst != lo {
                write_u32(bytes, mov_d_reg(dst, lo));
            }
            write_u32(bytes, ins_d1_d0(dst, hi));
        }
        // from GPRs: the low lane's fmov clears the high lane, which
        // the insert then writes
        VecInst::Pair(VecShape::I64x2, lo, hi) => {
            let dst = vec_home(alloc, result);
            let lo = materialize_operand_gpr(bytes, lo, OP_SCRATCH_LHS, alloc);
            let hi = materialize_operand_gpr(bytes, hi, OP_SCRATCH_RHS, alloc);
            write_u32(bytes, fmov_d_from_x(dst, lo));
            write_u32(bytes, ins_d1_x(dst, hi));
        }
        VecInst::FpToSi(v) => {
            let src = vec_operand(alloc, v);
            write_u32(bytes, fcvtzs_v2d(vec_home(alloc, result), src));
        }
        VecInst::FRintZ(v) => {
            let src = vec_operand(alloc, v);
            write_u32(bytes, frintz_v2d(vec_home(alloc, result), src));
        }
        // an f64 lane: lane 0 is the register's own D view, lane 1
        // the scalar dup
        VecInst::Lane(k, v) if matches!(alloc.of(result), Reg::Fpr(_) | Reg::SpillFpr(_)) => {
            let src = vec_operand(alloc, v);
            let (dst, spill) = alloc.def_fpr(result, FP_SCRATCH_RESULT);
            match k {
                0 if dst == src => {}
                0 => write_u32(bytes, mov_d_reg(dst, src)),
                _ => write_u32(bytes, dup_d_d1(dst, src)),
            }
            write_def_spill_fpr(bytes, spill, dst);
        }
        VecInst::Lane(k, v) => {
            let src = vec_operand(alloc, v);
            let (dst, spill) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
            write_u32(bytes, umov_x_d(dst, src, u32::from(*k)));
            write_def_spill_gpr(bytes, spill, dst);
        }
        VecInst::AnyLane(v) => {
            let src = vec_operand(alloc, v);
            let (dst, spill) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
            write_u32(bytes, addp_d_v2d(FP_SCRATCH_RESULT, src));
            write_u32(bytes, fmov_x_from_d(dst, FP_SCRATCH_RESULT));
            write_def_spill_gpr(bytes, spill, dst);
        }
        // the mask register is the one bsl rewrites; with the result
        // elsewhere, the insert forms keep the arm the result shares a
        // register with
        VecInst::Select(m, a, b) => {
            let (m, a, b) = (
                vec_operand(alloc, m),
                vec_operand(alloc, a),
                vec_operand(alloc, b),
            );
            let dst = vec_home(alloc, result);
            if dst == m {
                write_u32(bytes, bsl_v16b(dst, a, b));
            } else if dst == a {
                write_u32(bytes, bif_v16b(dst, b, m));
            } else if dst == b {
                write_u32(bytes, bit_v16b(dst, a, m));
            } else {
                write_u32(bytes, mov_d_reg(dst, m));
                write_u32(bytes, bsl_v16b(dst, a, b));
            }
        }
        VecInst::Ext(a, b) => {
            let (ra, rb) = (vec_operand(alloc, a), vec_operand(alloc, b));
            write_u32(bytes, ext_v16b_8(vec_home(alloc, result), ra, rb));
        }
        VecInst::BinOp(op, a, b) => {
            let (ra, rb) = (vec_operand(alloc, a), vec_operand(alloc, b));
            let dst = vec_home(alloc, result);
            write_u32(
                bytes,
                match op {
                    VecOp::Add => add_v2d(dst, ra, rb),
                    VecOp::Sub => sub_v2d(dst, ra, rb),
                    VecOp::And => and_v16b(dst, ra, rb),
                    VecOp::AndNot => bic_v16b(dst, ra, rb),
                    VecOp::Or => orr_v16b(dst, ra, rb),
                    VecOp::FAdd => fadd_v2d(dst, ra, rb),
                    VecOp::FSub => fsub_v2d(dst, ra, rb),
                    VecOp::FMul => fmul_v2d(dst, ra, rb),
                    VecOp::FDiv => fdiv_v2d(dst, ra, rb),
                    VecOp::FCmEq => fcmeq_v2d(dst, ra, rb),
                    VecOp::FCmGt => fcmgt_v2d(dst, ra, rb),
                    VecOp::FCmGe => fcmge_v2d(dst, ra, rb),
                    VecOp::CmEq => cmeq_v2d(dst, ra, rb),
                    VecOp::CmGt => cmgt_v2d(dst, ra, rb),
                    VecOp::CmGe => cmge_v2d(dst, ra, rb),
                },
            );
        }
        VecInst::Reduce(op, v) => {
            let src = vec_operand(alloc, v);
            let (dst, spill) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
            match op {
                VecOp::Add => {
                    write_u32(bytes, addp_d_v2d(FP_SCRATCH_RESULT, src));
                    write_u32(bytes, fmov_x_from_d(dst, FP_SCRATCH_RESULT));
                }
                VecOp::Or => {
                    write_u32(bytes, umov_x_d(OP_SCRATCH_TMP, src, 1));
                    write_u32(bytes, umov_x_d(dst, src, 0));
                    write_u32(bytes, orr_reg(dst, dst, OP_SCRATCH_TMP));
                }
                VecOp::Sub
                | VecOp::And
                | VecOp::AndNot
                | VecOp::FAdd
                | VecOp::FSub
                | VecOp::FMul
                | VecOp::FDiv
                | VecOp::FCmEq
                | VecOp::FCmGt
                | VecOp::FCmGe
                | VecOp::CmEq
                | VecOp::CmGt
                | VecOp::CmGe => panic!("vec.reduce.{} has no lowering", op.as_str()),
            }
            write_def_spill_gpr(bytes, spill, dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_function;
    use crate::enc;
    use crate::linear_scan::allocate_linear_scan;
    use torajs_core::ssa::{BinOp, FuncId, Function, InstKind, Terminator, Type};

    const V: Type = Type::Vec(VecShape::I64x2);

    fn val(v: ValueId) -> Operand {
        Operand::Value(v)
    }

    fn vec_inst(f: &mut Function, vi: VecInst, ty: Type) -> ValueId {
        f.append_inst(torajs_core::ssa::BlockId(0), InstKind::Vec(vi), ty, None)
    }

    /// `p[0..2] + p[2..4]`, or-ed with a splat of 5, reduced both ways.
    fn lanes_func() -> (Function, [ValueId; 8]) {
        let mut f = Function::new("lanes", Type::I64);
        let p = f.add_param(Type::Ptr, "p");
        let b = f.add_block();
        let a = f.append_inst(b, InstKind::Load(V, val(p), 0), V, None);
        let c = f.append_inst(b, InstKind::Load(V, val(p), 16), V, None);
        let s = vec_inst(&mut f, VecInst::BinOp(VecOp::Add, val(a), val(c)), V);
        let k = vec_inst(
            &mut f,
            VecInst::Splat(VecShape::I64x2, Operand::ConstI64(5)),
            V,
        );
        let o = vec_inst(&mut f, VecInst::BinOp(VecOp::Or, val(s), val(k)), V);
        let r = vec_inst(&mut f, VecInst::Reduce(VecOp::Add, val(o)), Type::I64);
        let q = vec_inst(&mut f, VecInst::Reduce(VecOp::Or, val(o)), Type::I64);
        let t = f.append_inst(
            b,
            InstKind::BinOp(BinOp::Add, val(r), val(q)),
            Type::I64,
            None,
        );
        f.set_term(b, Terminator::Ret(Some(val(t))));
        (f, [p, a, c, s, k, o, r, q])
    }

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn lane_ops_emit_q_forms_in_order() {
        let (func, [p, a, c, s, k, o, r, q]) = lanes_func();
        let alloc = allocate_linear_scan(&func);
        let fpr = |v: ValueId| vec_home(&alloc, v);
        let gpr = |v: ValueId| match alloc.of(v) {
            Reg::Gpr(g) => g,
            other => panic!("{other:?}"),
        };
        let expect = [
            enc::ldr_q_imm12(fpr(a), gpr(p), 0),
            enc::ldr_q_imm12(fpr(c), gpr(p), 16),
            enc::add_v2d(fpr(s), fpr(a), fpr(c)),
            enc::orr_v16b(fpr(o), fpr(s), fpr(k)),
            enc::addp_d_v2d(FP_SCRATCH_RESULT, fpr(o)),
            enc::fmov_x_from_d(gpr(r), FP_SCRATCH_RESULT),
            enc::umov_x_d(OP_SCRATCH_TMP, fpr(o), 1),
            enc::umov_x_d(gpr(q), fpr(o), 0),
            enc::orr_reg(gpr(q), gpr(q), OP_SCRATCH_TMP),
        ];
        let got = words(&compile_function(&func).bytes);
        let mut at = 0;
        for w in expect {
            at += got[at..]
                .iter()
                .position(|&g| g == w)
                .unwrap_or_else(|| panic!("missing {w:#010x} after word {at}"))
                + 1;
        }
    }

    /// `(a, b) & (trunc(p[0..2] / p[2..4]) == p[0..2] / p[2..4])`,
    /// reduced: the divisibility-mask chain over an i64 lane pair.
    #[test]
    fn mask_chain_emits_frintz_fcmeq_and_over_an_i64_pair() {
        const F: Type = Type::Vec(VecShape::F64x2);
        let mut f = Function::new("mask", Type::I64);
        let p = f.add_param(Type::Ptr, "p");
        let a = f.add_param(Type::I64, "a");
        let b = f.add_param(Type::I64, "b");
        let blk = f.add_block();
        let x = f.append_inst(blk, InstKind::Load(F, val(p), 0), F, None);
        let y = f.append_inst(blk, InstKind::Load(F, val(p), 16), F, None);
        let q = vec_inst(&mut f, VecInst::BinOp(VecOp::FDiv, val(x), val(y)), F);
        let t = vec_inst(&mut f, VecInst::FRintZ(val(q)), F);
        let m = vec_inst(&mut f, VecInst::BinOp(VecOp::FCmEq, val(t), val(q)), V);
        let s = vec_inst(&mut f, VecInst::Pair(VecShape::I64x2, val(a), val(b)), V);
        let k = vec_inst(&mut f, VecInst::BinOp(VecOp::And, val(m), val(s)), V);
        let r = vec_inst(&mut f, VecInst::Reduce(VecOp::Add, val(k)), Type::I64);
        f.set_term(blk, Terminator::Ret(Some(val(r))));
        let alloc = allocate_linear_scan(&f);
        let fpr = |v: ValueId| vec_home(&alloc, v);
        let gpr = |v: ValueId| match alloc.of(v) {
            Reg::Gpr(g) => g,
            other => panic!("{other:?}"),
        };
        let expect = [
            enc::fdiv_v2d(fpr(q), fpr(x), fpr(y)),
            enc::frintz_v2d(fpr(t), fpr(q)),
            enc::fcmeq_v2d(fpr(m), fpr(t), fpr(q)),
            enc::fmov_d_from_x(fpr(s), gpr(a)),
            enc::ins_d1_x(fpr(s), gpr(b)),
            enc::and_v16b(fpr(k), fpr(m), fpr(s)),
        ];
        let got = words(&compile_function(&f).bytes);
        let mut at = 0;
        for w in expect {
            at += got[at..]
                .iter()
                .position(|&g| g == w)
                .unwrap_or_else(|| panic!("missing {w:#010x} after word {at}"))
                + 1;
        }
    }

    /// Both loaded vectors are live across the call: each has a
    /// 16-byte slot at a 16-aligned offset, apart from the other's,
    /// and keeps its register.
    #[test]
    fn vectors_live_across_a_call_are_saved_whole() {
        let (mut func, [_, a, c, ..]) = lanes_func();
        let call = Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), Vec::new()),
            origin: None,
        };
        // between the loads and their use
        func.blocks[0].insts.insert(2, call);
        let alloc = allocate_linear_scan(&func);
        let saves = &alloc.vec_call_saves[&2];
        let mut regs: Vec<Fpr> = saves.iter().map(|s| s.0).collect();
        regs.sort_by_key(|f| f.idx());
        let mut want = vec![vec_home(&alloc, a), vec_home(&alloc, c)];
        want.sort_by_key(|f| f.idx());
        assert_eq!(regs, want);
        let (s0, s1) = (saves[0].1, saves[1].1);
        assert!(s0.is_multiple_of(16) && s1.is_multiple_of(16));
        assert!(s0.abs_diff(s1) >= 16);
        assert!(alloc.call_saves.get(&2).is_none_or(Vec::is_empty));
    }

    /// More f64 values live inside a vector's range than there are
    /// FPRs, each read more often than the vector: by spill weight the
    /// vector is the cheapest victim. The f64 values spill instead and
    /// the vector keeps its register.
    #[test]
    fn fpr_pressure_spills_f64_values_not_vectors() {
        let (mut func, [_, _, _, s, ..]) = lanes_func();
        let floats: Vec<ValueId> = (0..30)
            .map(|k| f64_value(&mut func, InstKind::SiToFp(Operand::ConstI64(k))))
            .collect();
        for _ in 0..2 {
            let mut acc = Operand::Value(floats[0]);
            for &f in &floats[1..] {
                let kind = InstKind::BinOp(BinOp::FAdd, acc, Operand::Value(f));
                acc = Operand::Value(f64_value(&mut func, kind));
            }
        }
        // move the f64 code between the def of `s` and its one use
        let insts = &mut func.blocks[0].insts;
        let tail: Vec<Inst> = insts.drain(8..).collect();
        insts.splice(3..3, tail);
        let alloc = allocate_linear_scan(&func);
        let spilled = |f: &ValueId| matches!(alloc.of(*f), Reg::SpillFpr(_));
        assert!(floats.iter().any(spilled));
        assert!(matches!(alloc.of(s), Reg::Fpr(_)));
        compile_function(&func);
    }

    /// The call sits in an arm the vector is dead on: its hull crosses
    /// the call and its live ranges do not, so it is not saved around
    /// the call. A value sharing its register in that arm would take
    /// the reload over its own value.
    ///
    /// ```text
    /// bb0: a = load p ; br c ? bb1 : bb2   ; slots 0, 1
    /// bb1: call f0 ; ret 0                 ; slots 2, 3
    /// bb2: r = reduce a ; ret r            ; slots 4, 5
    /// ```
    #[test]
    fn a_vector_dead_at_a_call_in_its_hull_is_not_saved_around_it() {
        let mut f = Function::new("hole", Type::I64);
        let p = f.add_param(Type::Ptr, "p");
        let c = f.add_param(Type::Bool, "c");
        let (b0, b1, b2) = (f.add_block(), f.add_block(), f.add_block());
        let a = f.append_inst(b0, InstKind::Load(V, val(p), 0), V, None);
        f.set_term(
            b0,
            Terminator::CondBr {
                cond: val(c),
                then_blk: b1,
                else_blk: b2,
            },
        );
        f.blocks[b1.0 as usize].insts.push(Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), Vec::new()),
            origin: None,
        });
        f.set_term(b1, Terminator::Ret(Some(Operand::ConstI64(0))));
        let reduce = InstKind::Vec(VecInst::Reduce(VecOp::Add, val(a)));
        let r = f.append_inst(b2, reduce, Type::I64, None);
        f.set_term(b2, Terminator::Ret(Some(val(r))));
        let alloc = allocate_linear_scan(&f);
        assert!(matches!(alloc.of(a), Reg::Fpr(_)));
        assert!(alloc.vec_call_saves.get(&2).is_none_or(Vec::is_empty));
        compile_function(&f);
    }

    fn f64_value(f: &mut Function, kind: InstKind) -> ValueId {
        f.append_inst(torajs_core::ssa::BlockId(0), kind, Type::F64, None)
    }
}
