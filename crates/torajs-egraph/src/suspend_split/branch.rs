//! A branch on the frame's recorded state to one block per value.
//!
//! The IR has no multiway terminator, so the choice is a tree of
//! compares: a short chain while there are few targets (each one
//! compare further down), a balanced binary search once there are
//! more, so reaching any of `n` targets costs about log2(n) compares
//! rather than up to `n`.

use torajs_core::ssa::{BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type};

use super::cut::fresh_block;
use super::spill::new_value;

/// Up to this many targets the compares form a chain.
const CHAIN_MAX: usize = 4;

/// End block `from` with a branch on `state` to the block of each
/// `(value, block)` of `targets` (ascending values). `rest` takes any
/// other value; without it, the values listed are the only ones
/// `state` can hold.
pub(super) fn on_state(
    f: &mut Function,
    from: BlockId,
    state: Operand,
    targets: &[(i64, BlockId)],
    rest: Option<BlockId>,
) {
    if targets.len() <= CHAIN_MAX {
        return chain(f, from, state, targets, rest);
    }
    let mid = targets.len() / 2;
    let below = fresh_block(f);
    let above = fresh_block(f);
    let c = compare(f, from, IPred::Slt, state, targets[mid].0);
    let term = Terminator::CondBr {
        cond: c,
        then_blk: below,
        else_blk: above,
    };
    end(f, from, term);
    on_state(f, below, state, &targets[..mid], rest);
    on_state(f, above, state, &targets[mid..], rest);
}

fn chain(
    f: &mut Function,
    mut from: BlockId,
    state: Operand,
    targets: &[(i64, BlockId)],
    rest: Option<BlockId>,
) {
    if targets.is_empty() {
        let r = rest.expect("internal error: a state branch with no target");
        return end(f, from, Terminator::Br(r));
    }
    for (n, &(k, target)) in targets.iter().enumerate() {
        let next = match (n + 1 == targets.len(), rest) {
            (true, None) => return end(f, from, Terminator::Br(target)),
            (true, Some(r)) => r,
            (false, _) => fresh_block(f),
        };
        let c = compare(f, from, IPred::Eq, state, k);
        let term = Terminator::CondBr {
            cond: c,
            then_blk: target,
            else_blk: next,
        };
        end(f, from, term);
        from = next;
    }
}

fn compare(f: &mut Function, b: BlockId, pred: IPred, state: Operand, k: i64) -> Operand {
    let c = new_value(f, Type::Bool);
    f.blocks[b.0 as usize].insts.push(Inst {
        result: Some(c),
        kind: InstKind::ICmp(pred, state, Operand::ConstI64(k)),
        origin: None,
    });
    Operand::Value(c)
}

fn end(f: &mut Function, b: BlockId, term: Terminator) {
    f.blocks[b.0 as usize].term = term;
}
