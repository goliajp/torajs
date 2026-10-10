//! The search loop: its entry from the original latch, the lanes and
//! group test, the group's divides, its exit and its step.

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst,
    VecOp, VecShape,
};

use super::super::emit::{F64X2, I64X2, new_value, push};
use super::{LANES, PROLOGUE, SearchPlan};

/// Builds the search loop; answers its header `F`.
pub(in crate::unroll_pair) fn apply(func: &mut Function, sp: &SearchPlan) -> BlockId {
    let v = Operand::Value;
    let c = sp.step;
    // the original loop runs its first steps; its latch enters the
    // search once, when the counter has taken them
    let e = new_value(func, Type::I64);
    let first = InstKind::BinOp(BinOp::Add, v(sp.iv), Operand::ConstI64(PROLOGUE * c));
    push(func, sp.preheader, e, first);
    let s = new_block(func);
    let enter = new_value(func, Type::Bool);
    push(
        func,
        sp.latch,
        enter,
        InstKind::ICmp(IPred::Eq, v(sp.iv), v(e)),
    );
    func.blocks[sp.latch.0 as usize].term = Terminator::CondBr {
        cond: v(enter),
        then_blk: s,
        else_blk: sp.header,
    };
    let (xv, cells, kv) = entry_lanes(func, s, sp);
    let f = new_block(func);
    let fl = new_block(func);
    func.blocks[s.0 as usize].term = group_test(func, s, sp, f);
    // F: a dividing lane is the loop's exit; the first one is taken
    let (hit, masks) = any_lane_divides(func, f, xv, &cells);
    let fh = new_block(func);
    func.blocks[f.0 as usize].term = Terminator::CondBr {
        cond: v(hit),
        then_blk: fh,
        else_blk: fl,
    };
    leave_at_first_lane(func, fh, sp, &masks);
    // FL: the group's steps, then the next group's test
    let next = new_value(func, Type::I64);
    let add = InstKind::BinOp(BinOp::Add, v(sp.iv), Operand::ConstI64(LANES * c));
    push(func, fl, next, add);
    push(func, fl, sp.iv, InstKind::Copy(Type::I64, v(next)));
    for &cell in &cells {
        let y = new_value(func, F64X2);
        push(
            func,
            fl,
            y,
            vec(VecInst::BinOp(VecOp::FAdd, v(cell), v(kv))),
        );
        push(func, fl, cell, InstKind::Copy(F64X2, v(y)));
    }
    func.blocks[fl.0 as usize].term = group_test(func, fl, sp, f);
    f
}

/// The dividend lanes, the divisor lane cells (the counter and the
/// group's next divisors) and the group step's splat, made in `pre`.
fn entry_lanes(
    func: &mut Function,
    pre: BlockId,
    sp: &SearchPlan,
) -> (ValueId, Vec<ValueId>, ValueId) {
    let v = Operand::Value;
    let c = sp.step;
    let xv = new_value(func, F64X2);
    push(
        func,
        pre,
        xv,
        vec(VecInst::Splat(VecShape::F64x2, sp.dividend)),
    );
    let y0 = new_value(func, Type::F64);
    push(func, pre, y0, InstKind::SiToFp(v(sp.iv)));
    let lane = |func: &mut Function, k: i64| {
        if k == 0 {
            return y0;
        }
        let y = new_value(func, Type::F64);
        let kind = InstKind::BinOp(BinOp::FAdd, v(y0), Operand::ConstF64((k * c) as f64));
        push(func, pre, y, kind);
        y
    };
    let cells: Vec<ValueId> = (0..LANES / 2)
        .map(|g| {
            let (lo, hi) = (lane(func, 2 * g), lane(func, 2 * g + 1));
            let pair = new_value(func, F64X2);
            push(
                func,
                pre,
                pair,
                vec(VecInst::Pair(VecShape::F64x2, v(lo), v(hi))),
            );
            let cell = new_value(func, F64X2);
            push(func, pre, cell, InstKind::Copy(F64X2, v(pair)));
            cell
        })
        .collect();
    let group = Operand::ConstF64((LANES * c) as f64);
    let kv = new_value(func, F64X2);
    push(func, pre, kv, vec(VecInst::Splat(VecShape::F64x2, group)));
    (xv, cells, kv)
}

fn vec(vi: VecInst) -> InstKind {
    InstKind::Vec(vi)
}

fn new_block(func: &mut Function) -> BlockId {
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts: Vec::new(),
        term: Terminator::Unreachable,
    });
    id
}

/// The header test at the group's last divisor, appended to `b`: on
/// to `f` when it holds, else to the original loop.
fn group_test(func: &mut Function, b: BlockId, sp: &SearchPlan, f: BlockId) -> Terminator {
    let v = Operand::Value;
    let t = &sp.test;
    let last = new_value(func, Type::I64);
    let at = Operand::ConstI64((LANES - 1) * sp.step);
    push(func, b, last, InstKind::BinOp(BinOp::Add, v(sp.iv), at));
    let x = if t.square {
        let sq = new_value(func, Type::I64);
        push(func, b, sq, InstKind::BinOp(BinOp::Mul, v(last), v(last)));
        sq
    } else {
        last
    };
    let (lhs, rhs) = if t.iv_lhs {
        (v(x), t.bound)
    } else {
        (t.bound, v(x))
    };
    let c = new_value(func, Type::Bool);
    push(func, b, c, InstKind::ICmp(t.pred, lhs, rhs));
    let (then_blk, else_blk) = if t.stay_then {
        (f, sp.header)
    } else {
        (sp.header, f)
    };
    Terminator::CondBr {
        cond: v(c),
        then_blk,
        else_blk,
    }
}

/// Appends to `f` the test that some lane of `xv / cell`, over the
/// cells, is integral; answers the test and each cell's lane mask.
fn any_lane_divides(
    func: &mut Function,
    f: BlockId,
    xv: ValueId,
    cells: &[ValueId],
) -> (ValueId, Vec<ValueId>) {
    let v = Operand::Value;
    let mut masks = Vec::new();
    let mut mask = None;
    for &cell in cells {
        let q = new_value(func, F64X2);
        push(func, f, q, vec(VecInst::BinOp(VecOp::FDiv, v(xv), v(cell))));
        let t = new_value(func, F64X2);
        push(func, f, t, vec(VecInst::FRintZ(v(q))));
        let m = new_value(func, I64X2);
        push(func, f, m, vec(VecInst::BinOp(VecOp::FCmEq, v(t), v(q))));
        masks.push(m);
        mask = Some(match mask {
            None => m,
            Some(prev) => {
                let or = new_value(func, I64X2);
                push(func, f, or, vec(VecInst::BinOp(VecOp::Or, v(prev), v(m))));
                or
            }
        });
    }
    let mask = mask.expect("a group has a vector");
    let sum = new_value(func, Type::I64);
    push(func, f, sum, vec(VecInst::Reduce(VecOp::Add, v(mask))));
    let hit = new_value(func, Type::Bool);
    push(
        func,
        f,
        hit,
        InstKind::ICmp(IPred::Ne, v(sum), Operand::ConstI64(0)),
    );
    (hit, masks)
}

/// `fh`: the counter moved to the first dividing lane, then the
/// divisibility exit. Every lane of the group passed the header test,
/// so the original loop leaves there, with that counter.
fn leave_at_first_lane(func: &mut Function, fh: BlockId, sp: &SearchPlan, masks: &[ValueId]) {
    let v = Operand::Value;
    let mut off = Operand::ConstI64((LANES - 1) * sp.step);
    for k in (0..LANES - 1).rev() {
        let lane = new_value(func, Type::I64);
        let m = masks[(k / 2) as usize];
        push(func, fh, lane, vec(VecInst::Lane((k % 2) as u8, v(m))));
        let set = new_value(func, Type::Bool);
        let ne = InstKind::ICmp(IPred::Ne, v(lane), Operand::ConstI64(0));
        push(func, fh, set, ne);
        let pick = new_value(func, Type::I64);
        let at = Operand::ConstI64(k * sp.step);
        push(func, fh, pick, InstKind::Select(Type::I64, v(set), at, off));
        off = v(pick);
    }
    let at = new_value(func, Type::I64);
    push(func, fh, at, InstKind::BinOp(BinOp::Add, v(sp.iv), off));
    push(func, fh, sp.iv, InstKind::Copy(Type::I64, v(at)));
    func.blocks[fh.0 as usize].term = Terminator::Br(sp.found);
}
