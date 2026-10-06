//! Control-flow questions the widening pass asks of the scalar loop.

use crate::id_hash::HashSet;

use torajs_core::ssa::{BlockId, Function, Operand, Terminator, ValueId, visit_value_operands};

pub(super) fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let out = match term {
        Terminator::Br(b) => [Some(*b), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(*then_blk), Some(*else_blk)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    };
    out.into_iter().flatten()
}

/// Is `v` read on some path from the top of `from` before a def of it?
pub(super) fn live_in(func: &Function, from: BlockId, v: ValueId) -> bool {
    let mut seen: HashSet<BlockId> = HashSet::default();
    let mut work = vec![from];
    while let Some(b) = work.pop() {
        if !seen.insert(b) {
            continue;
        }
        let block = &func.blocks[b.0 as usize];
        let mut killed = false;
        for inst in &block.insts {
            let mut used = false;
            visit_value_operands(&inst.kind, |x| used |= x == v);
            if used {
                return true;
            }
            if inst.result == Some(v) {
                killed = true;
                break;
            }
        }
        if killed {
            continue;
        }
        let term_reads = match &block.term {
            Terminator::CondBr { cond, .. } => *cond == Operand::Value(v),
            Terminator::Ret(Some(op)) => *op == Operand::Value(v),
            _ => false,
        };
        if term_reads {
            return true;
        }
        work.extend(successors(&block.term));
    }
    false
}
