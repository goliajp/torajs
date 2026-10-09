//! The edges a group leaves by: the guards lanes copy from the loop,
//! and the group bound computed on entry.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
};

use super::Plan;
use super::clone::{new_value, push};

/// The guard closing block `b` tests a shared value: lane 0's copy
/// answers for every lane.
pub(super) fn guard_shared(func: &Function, p: &Plan, b: BlockId) -> bool {
    matches!(
        func.blocks[b.0 as usize].term,
        Terminator::CondBr { cond: Operand::Value(c), .. } if p.shared.contains(&c)
    )
}

/// A pre or inner-body guard leaving the loop, cloned for one lane: the
/// group falls back to the original header (nothing is written yet)
/// when it fires. Returns the block the lane continues in.
pub(super) fn lane_guard(
    func: &mut Function,
    p: &Plan,
    b: BlockId,
    cur: BlockId,
    map: &HashMap<ValueId, ValueId>,
) -> BlockId {
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        ..
    } = func.blocks[b.0 as usize].term
    else {
        return cur;
    };
    if b == p.header {
        return cur;
    }
    let next = func.add_block();
    let cond = Operand::Value(map.get(&c).copied().unwrap_or(c));
    let (then_blk, else_blk) = if p.body.contains(&then_blk) {
        (next, p.header)
    } else {
        (p.header, next)
    };
    func.blocks[cur.0 as usize].term = Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
    next
}

/// `PH`: the group bound `bk`, and whether computing it wrapped. A
/// constant bound was checked when planning.
pub(super) fn entry_block(func: &mut Function, p: &Plan, ph: BlockId, g: BlockId) -> Operand {
    let span = p.step * (p.lanes as i64 - 1);
    match p.bound {
        Operand::ConstI64(n) => {
            func.blocks[ph.0 as usize].term = Terminator::Br(g);
            Operand::ConstI64(n - span)
        }
        bound => {
            let bk = new_value(func, Type::I64);
            push(
                func,
                ph,
                bk,
                InstKind::BinOp(BinOp::Sub, bound, Operand::ConstI64(span)),
            );
            let ok = new_value(func, Type::Bool);
            push(
                func,
                ph,
                ok,
                InstKind::ICmp(IPred::Slt, Operand::Value(bk), bound),
            );
            func.blocks[ph.0 as usize].term = Terminator::CondBr {
                cond: Operand::Value(ok),
                then_blk: g,
                else_blk: p.header,
            };
            Operand::Value(bk)
        }
    }
}
