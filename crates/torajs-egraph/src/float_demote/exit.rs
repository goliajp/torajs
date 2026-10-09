//! Side exits and check chains for the guards `apply` installs.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::guard::{GuardCheck, Test};
use super::push_value;

/// A side-exit block: write back every cloned value live at the slow
/// continuation, then branch into it. Demoted values bridge through
/// `sitofp` (exact by the guard invariant), intermediates copy.
pub(super) fn build_side_exit(
    func: &mut Function,
    target: BlockId,
    live_in: &HashMap<BlockId, HashSet<ValueId>>,
    inverse: &HashMap<ValueId, ValueId>,
    set: &HashSet<ValueId>,
) -> BlockId {
    let mut live: Vec<ValueId> = live_in
        .get(&target)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default();
    live.sort_by_key(|v| v.0);
    let mut insts: Vec<Inst> = Vec::with_capacity(live.len());
    for f in live {
        let o = inverse[&f];
        let kind = if set.contains(&o) {
            InstKind::SiToFp(Operand::Value(o))
        } else {
            InstKind::Copy(func.values[o.0 as usize].ty, Operand::Value(o))
        };
        insts.push(Inst {
            result: Some(f),
            kind,
            origin: None,
        });
    }
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts,
        term: Terminator::Br(target),
    });
    id
}

/// A chain of check blocks: each violation branches to `on_fail`,
/// passing all checks reaches `on_pass`. Returns the chain head.
pub(super) fn build_check_chain(
    func: &mut Function,
    checks: &[GuardCheck],
    on_fail: BlockId,
    on_pass: BlockId,
) -> BlockId {
    debug_assert!(!checks.is_empty());
    let head = BlockId(func.blocks.len() as u32);
    for (i, c) in checks.iter().enumerate() {
        let id = BlockId(func.blocks.len() as u32);
        let next = if i + 1 == checks.len() {
            on_pass
        } else {
            BlockId(id.0 + 1)
        };
        func.blocks.push(Block {
            id,
            insts: Vec::new(),
            term: Terminator::Br(next),
        });
        let cond = push_check(func, id, c);
        func.blocks[id.0 as usize].term = Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: on_fail,
            else_blk: next,
        };
    }
    head
}

/// Append one check to `block`; the returned bool is true on
/// violation.
pub(super) fn push_check(func: &mut Function, block: BlockId, c: &GuardCheck) -> ValueId {
    let cond = push_value(&mut func.values, Type::Bool);
    let mut insts: Vec<Inst> = Vec::new();
    let kind = match c.test {
        Test::Above(bound) => InstKind::ICmp(IPred::Sgt, c.operand, Operand::ConstI64(bound)),
        Test::Below(bound) => InstKind::ICmp(IPred::Slt, c.operand, Operand::ConstI64(bound)),
        Test::HighBits(k) => {
            let high = push_value(&mut func.values, Type::I64);
            insts.push(Inst {
                result: Some(high),
                kind: InstKind::BinOp(BinOp::LShr, c.operand, Operand::ConstI64(k as i64)),
                origin: None,
            });
            InstKind::ICmp(IPred::Ne, Operand::Value(high), Operand::ConstI64(0))
        }
    };
    insts.push(Inst {
        result: Some(cond),
        kind,
        origin: None,
    });
    func.blocks[block.0 as usize].insts.extend(insts);
    cond
}
