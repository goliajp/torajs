//! Small CFG queries shared by the versioning planner (`plan`) and
//! installer (`apply`): successor/predecessor maps, per-value
//! def/use block indexes, and terminator helpers.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, Operand, Terminator, ValueId};

use crate::rc_peephole::visit_value_operands;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

pub(crate) fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
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
        .filter(|l| l.contains(&b))
        .min_by_key(|l| l.body.len())
        .map(|l| (l.body.len(), l.header))
}

/// Per-value (def blocks, use blocks). Terminator operands count as
/// uses.
pub(super) fn collect_value_blocks(
    func: &Function,
) -> HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)> {
    let mut m: HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)> = HashMap::default();
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

pub(crate) fn multi_def_values(func: &Function) -> HashSet<ValueId> {
    let mut seen: HashSet<ValueId> = HashSet::default();
    let mut multi: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && !seen.insert(r)
            {
                multi.insert(r);
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
    let mut touched: HashSet<ValueId> = HashSet::default();
    for b in region {
        let block = &func.blocks[b.0 as usize];
        for inst in &block.insts {
            if let Some(r) = inst.result
                && set.contains(&r)
            {
                touched.insert(r);
            }
            visit_value_operands(&inst.kind, |v| {
                if set.contains(&v) {
                    touched.insert(v);
                }
            });
        }
        if let Some(v) = term_operand_value(&block.term)
            && set.contains(&v)
        {
            touched.insert(v);
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
    if let Some(v) = term_operand_value(&block.term)
        && fresh.contains(&v)
    {
        hit = true;
    }
    hit
}

pub(crate) fn term_operand_value(term: &Terminator) -> Option<ValueId> {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } => Some(*v),
        Terminator::Ret(Some(Operand::Value(v))) => Some(*v),
        _ => None,
    }
}

pub(crate) fn remap_term_operand(
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

pub(crate) fn retarget(term: Terminator, block_map: &HashMap<BlockId, BlockId>) -> Terminator {
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

pub(crate) fn retarget_edge(term: &mut Terminator, from: BlockId, to: BlockId) {
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

/// The function's shape as planning reads it. Evicting values from
/// the set leaves the function as it was, so one of these serves
/// every planning round until a split changes the blocks.
pub(crate) struct PlanCfg {
    pub(super) dom: DominatorTree,
    pub(super) loops: LoopAnalysis,
    pub(super) value_blocks: HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)>,
    pub(super) preds: Vec<Vec<BlockId>>,
    pub(super) cells: HashSet<ValueId>,
}

impl PlanCfg {
    pub(crate) fn of(func: &Function) -> PlanCfg {
        let dom = DominatorTree::compute(func);
        let loops = LoopAnalysis::compute(func, &dom);
        PlanCfg {
            dom,
            loops,
            value_blocks: collect_value_blocks(func),
            preds: pred_map(func),
            cells: multi_def_values(func),
        }
    }
}

/// The split point of a mixed pre-chain block: the position of its
/// first in-set def. Valid when the block is absorbed pre-chain (not
/// loop body), something precedes the point, and every def at or
/// after it is region-clean — in-set (bridged when escaping), a
/// shared multi-def cell, or used only inside the region and this
/// block. The head segment then leaves the region on re-plan.
pub(super) fn tail_split_point(
    func: &Function,
    set: &HashSet<ValueId>,
    cells: &HashSet<ValueId>,
    value_blocks: &HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)>,
    region: &HashSet<BlockId>,
    body: &[BlockId],
    d: BlockId,
) -> Option<usize> {
    if !region.contains(&d) || body.contains(&d) {
        return None;
    }
    let insts = &func.blocks[d.0 as usize].insts;
    let p = insts
        .iter()
        .position(|i| i.result.is_some_and(|r| set.contains(&r)))?;
    if p == 0 {
        return None;
    }
    for inst in &insts[p..] {
        let Some(r) = inst.result else { continue };
        if set.contains(&r) || cells.contains(&r) {
            continue;
        }
        let clean = value_blocks
            .get(&r)
            .is_none_or(|(_, uses)| uses.iter().all(|b| region.contains(b) || *b == d));
        if !clean {
            return None;
        }
    }
    Some(p)
}
