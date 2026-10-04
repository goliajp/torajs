//! `Type::Vec` values: Q-register loads, stores, copies and the lane
//! arithmetic of `InstKind::Vec`.
//!
//! A vector value always has a register home — the allocator rejects
//! a spilled or call-crossing one (`vec_homes`), since every save and
//! spill path moves 8 bytes.

use torajs_core::ssa::{Inst, Operand, ValueId, VecInst, VecOp};

use super::operand::materialize_operand_gpr;
use super::{
    FP_SCRATCH_RESULT, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_TMP, write_def_spill_gpr,
    write_u32,
};
use crate::enc::{
    add_imm, add_v2d, addp_d_v2d, dup_2d_x, fmov_x_from_d, ldr_q_imm12, ldur_q_imm9, mov_d_reg,
    movi_2d_zero, orr_reg, orr_v16b, str_q_imm12, stur_q_imm9, umov_x_d,
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
        VecInst::Splat(_, scalar) => {
            let src = materialize_operand_gpr(bytes, scalar, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, dup_2d_x(vec_home(alloc, result), src));
        }
        VecInst::BinOp(op, a, b) => {
            let (ra, rb) = (vec_operand(alloc, a), vec_operand(alloc, b));
            let dst = vec_home(alloc, result);
            write_u32(
                bytes,
                match op {
                    VecOp::Add => add_v2d(dst, ra, rb),
                    VecOp::Or => orr_v16b(dst, ra, rb),
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
    use torajs_core::ssa::{BinOp, FuncId, Function, InstKind, Terminator, Type, VecShape};

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

    #[test]
    #[should_panic(expected = "live across a call")]
    fn vector_live_across_a_call_is_refused() {
        let (mut func, _) = lanes_func();
        let call = Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), Vec::new()),
            origin: None,
        };
        // between the loads and their use
        func.blocks[0].insts.insert(2, call);
        allocate_linear_scan(&func);
    }
}
