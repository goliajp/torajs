//! Function-wide facts a join's plan reads: the single definitions,
//! operand counts and predecessors.

use crate::id_hash::HashMap;

use torajs_core::ssa::{Function, Operand, Terminator, ValueId, visit_value_operands};

pub(super) type Defs = HashMap<ValueId, (usize, usize)>;

/// The single definitions, and operand occurrences per value.
pub(super) fn defs_and_uses(func: &Function) -> (Defs, HashMap<ValueId, u32>) {
    let mut count: HashMap<ValueId, u32> = HashMap::default();
    let mut at: Defs = HashMap::default();
    let mut uses: HashMap<ValueId, u32> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (i, inst) in block.insts.iter().enumerate() {
            visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
            if let Some(r) = inst.result {
                *count.entry(r).or_insert(0) += 1;
                at.insert(r, (b, i));
            }
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *uses.entry(*v).or_insert(0) += 1,
            _ => {}
        }
    }
    at.retain(|v, _| count.get(v) == Some(&1));
    (at, uses)
}

pub(super) fn predecessors(func: &Function) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); func.blocks.len()];
    for (b, block) in func.blocks.iter().enumerate() {
        let (x, y) = match &block.term {
            Terminator::Br(t) => (Some(*t), None),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => (Some(*then_blk), Some(*else_blk)),
            Terminator::Ret(_) | Terminator::Unreachable => (None, None),
        };
        for t in x.into_iter().chain(y) {
            let list: &mut Vec<usize> = &mut preds[t.0 as usize];
            if !list.contains(&b) {
                list.push(b);
            }
        }
    }
    preds
}
