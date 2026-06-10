//! Small CFG queries shared by the versioning planner (`plan`) and
//! installer (`apply`): successor/predecessor maps, per-value
//! def/use block indexes, and terminator helpers.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, Operand, Terminator, ValueId};

use crate::loop_analysis::LoopAnalysis;
use crate::rc_peephole::visit_value_operands;

pub(super) fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let mut out: [Option<BlockId>; 2] = [None, None];
    match term {
        Terminator::Br(b) => out[0] = Some(*b),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            out[0] = Some(*then_blk);
            out[1] = Some(*else_blk);
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
    out.into_iter().flatten()
}

pub(super) fn pred_map(func: &Function) -> Vec<Vec<BlockId>> {
    let mut preds = vec![Vec::new(); func.blocks.len()];
    for b in &func.blocks {
        for s in successors(&b.term) {
            preds[s.0 as usize].push(b.id);
        }
    }
    preds
}

pub(super) fn innermost_loop(loops: &LoopAnalysis, b: BlockId) -> Option<(usize, BlockId)> {
    loops
        .loops()
        .iter()
        .filter(|l| l.body.contains(&b))
        .min_by_key(|l| l.body.len())
        .map(|l| (l.body.len(), l.header))
}

/// Per-value (def blocks, use blocks). Terminator operands count as
/// uses.
pub(super) fn collect_value_blocks(
    func: &Function,
) -> HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)> {
    let mut m: HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                let e = m.entry(r).or_default();
                if !e.0.contains(&block.id) {
                    e.0.push(block.id);
                }
            }
            visit_value_operands(&inst.kind, |v| {
                let e = m.entry(v).or_default();
                if !e.1.contains(&block.id) {
                    e.1.push(block.id);
                }
            });
        }
        if let Some(v) = term_operand_value(&block.term) {
            let e = m.entry(v).or_default();
            if !e.1.contains(&block.id) {
                e.1.push(block.id);
            }
        }
    }
    m
}

pub(super) fn multi_def_values(func: &Function) -> HashSet<ValueId> {
    let mut seen: HashSet<ValueId> = HashSet::new();
    let mut multi: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if !seen.insert(r) {
                    multi.insert(r);
                }
            }
        }
    }
    multi
}

/// In-set values defined or used inside the region.
pub(super) fn touched_values(
    func: &Function,
    set: &HashSet<ValueId>,
    region: &HashSet<BlockId>,
) -> HashSet<ValueId> {
    let mut touched: HashSet<ValueId> = HashSet::new();
    for b in region {
        let block = &func.blocks[b.0 as usize];
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if set.contains(&r) {
                    touched.insert(r);
                }
            }
            visit_value_operands(&inst.kind, |v| {
                if set.contains(&v) {
                    touched.insert(v);
                }
            });
        }
        if let Some(v) = term_operand_value(&block.term) {
            if set.contains(&v) {
                touched.insert(v);
            }
        }
    }
    touched
}

pub(super) fn block_touches(func: &Function, b: BlockId, fresh: &HashSet<ValueId>) -> bool {
    let block = &func.blocks[b.0 as usize];
    let mut hit = false;
    for inst in &block.insts {
        if inst.result.is_some_and(|r| fresh.contains(&r)) {
            hit = true;
        }
        visit_value_operands(&inst.kind, |v| {
            if fresh.contains(&v) {
                hit = true;
            }
        });
    }
    if let Some(v) = term_operand_value(&block.term) {
        if fresh.contains(&v) {
            hit = true;
        }
    }
    hit
}

pub(super) fn term_operand_value(term: &Terminator) -> Option<ValueId> {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } => Some(*v),
        Terminator::Ret(Some(Operand::Value(v))) => Some(*v),
        _ => None,
    }
}

pub(super) fn remap_term_operand(
    term: Terminator,
    op_map: &HashMap<ValueId, Operand>,
) -> Terminator {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            then_blk,
            else_blk,
        } if op_map.contains_key(&v) => Terminator::CondBr {
            cond: op_map[&v],
            then_blk,
            else_blk,
        },
        Terminator::Ret(Some(Operand::Value(v))) if op_map.contains_key(&v) => {
            Terminator::Ret(Some(op_map[&v]))
        }
        other => other,
    }
}

pub(super) fn retarget(term: Terminator, block_map: &HashMap<BlockId, BlockId>) -> Terminator {
    let map = |b: BlockId| block_map.get(&b).copied().unwrap_or(b);
    match term {
        Terminator::Br(b) => Terminator::Br(map(b)),
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } => Terminator::CondBr {
            cond,
            then_blk: map(then_blk),
            else_blk: map(else_blk),
        },
        other => other,
    }
}

pub(super) fn retarget_edge(term: &mut Terminator, from: BlockId, to: BlockId) {
    match term {
        Terminator::Br(b) if *b == from => *b = to,
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            if *then_blk == from {
                *then_blk = to;
            }
            if *else_blk == from {
                *else_blk = to;
            }
        }
        _ => {}
    }
}
