//! A triangle: a header whose one arm is the join itself and whose
//! other arm is a straight-line chain into it that only sets cells.
//!
//! The diamond conversion in the parent module needs a value defined
//! once in each arm. A conditional assignment to a cell that lives
//! across the branch — `if (holes == 0) flags &= ~HAS_HOLES` — has one
//! arm empty, and the cell has defs elsewhere too, so it stayed a
//! branch. On the common path that branch is taken and the arm's own
//! `br` back is taken again: two fetch redirects where a csel costs
//! none, in a loop whose iteration is bounded by taken branches
//! (objlit-optional-field-1m runs eight of them in nine cycles).
//! Each cell takes `select cond, <the arm's value>, <the cell>` through
//! a fresh value and a copy: the cell's own value stands for the arm
//! not taken. The cell must hold a value on every path into the header
//! for that read to mean anything: every path from the entry sets it
//! somewhere before the header (the two arms of an earlier branch
//! each setting it count, as a φ's inputs would).

use torajs_core::ssa::{BlockId, Function, Inst, InstKind, Operand, Terminator, ValueId};

use super::arm::{arm_chain, plan_arm};
use super::{SelectFormStats, Touched, plan_cells};
use crate::float_demote::push_value;

/// The predecessors of every block, by position, built once per
/// formation round the first time a triangle asks and read by every
/// candidate after.
pub(super) struct Preds {
    /// `list[start[b]..start[b + 1]]` are the predecessors of block `b`.
    start: Vec<u32>,
    list: Vec<u32>,
}

impl Preds {
    pub(super) fn compute(func: &Function) -> Self {
        let n = func.blocks.len();
        let succs = |block: &torajs_core::ssa::Block| match &block.term {
            Terminator::Br(t) => [Some(*t), None],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => [Some(*then_blk), Some(*else_blk)],
            Terminator::Ret(_) | Terminator::Unreachable => [None, None],
        };
        let mut start = vec![0u32; n + 1];
        for t in func.blocks.iter().flat_map(succs).flatten() {
            start[t.0 as usize + 1] += 1;
        }
        for b in 0..n {
            start[b + 1] += start[b];
        }
        let mut fill = start.clone();
        let mut list = vec![0u32; start[n] as usize];
        for (b, block) in func.blocks.iter().enumerate() {
            for t in succs(block).into_iter().flatten() {
                let slot = &mut fill[t.0 as usize];
                list[*slot as usize] = b as u32;
                *slot += 1;
            }
        }
        Self { start, list }
    }

    fn of(&self, b: usize) -> &[u32] {
        &self.list[self.start[b] as usize..self.start[b + 1] as usize]
    }

    /// Every path from the entry to block `h` sets `v` before `h`: no
    /// walk back from `h` over predecessors reaches the entry without
    /// passing a block that sets it. A block nothing reaches ends its
    /// walk, as a path that never runs sets everything.
    pub(super) fn set_on_every_path(&self, func: &Function, v: ValueId, h: usize) -> bool {
        if self.of(h).is_empty() {
            return false;
        }
        let sets = |b: usize| func.blocks[b].insts.iter().any(|i| i.result == Some(v));
        let mut seen = vec![false; func.blocks.len()];
        let mut work: Vec<u32> = self.of(h).to_vec();
        while let Some(b) = work.pop() {
            let b = b as usize;
            if std::mem::replace(&mut seen[b], true) || sets(b) {
                continue;
            }
            if b == 0 {
                return false;
            }
            work.extend_from_slice(self.of(b));
        }
        true
    }
}

/// Where the plain branches from `first` lead: the first block with
/// other predecessors, as `arm_chain` would end, without collecting
/// the blocks (or checking the arm's length, which `arm_chain` does).
fn chain_end(
    func: &Function,
    first: BlockId,
    pred_count: impl Fn(BlockId) -> u32,
) -> Option<BlockId> {
    let mut b = first;
    for _ in 0..func.blocks.len() {
        let Terminator::Br(next) = func.blocks[b.0 as usize].term else {
            return None;
        };
        if pred_count(next) != 1 {
            return Some(next);
        }
        b = next;
    }
    None
}

/// Convert the triangle at header `h` (`cond_br cond, t, e`), if it is
/// one; answers whether anything changed.
pub(super) fn try_form(
    func: &mut Function,
    h: usize,
    (cond, t_id, e_id): (&Operand, BlockId, BlockId),
    (pred_count, def_count): (&impl Fn(BlockId) -> u32, &impl Fn(ValueId) -> u32),
    preds: &mut Option<Preds>,
    touched: &mut Touched,
    stats: &mut SelectFormStats,
) -> bool {
    // the chain is collected only once its end says it is a triangle:
    // this runs on every header of every round, and few are one
    let chain_into = |arm: BlockId, join: BlockId| {
        (pred_count(arm) == 1 && chain_end(func, arm, pred_count) == Some(join))
            .then(|| arm_chain(func, arm, pred_count))
            .flatten()
            .filter(|(_, j)| *j == join)
            .map(|(blks, _)| blks)
    };
    let (blks, join, arm_is_then) = if let Some(b) = chain_into(t_id, e_id) {
        (b, e_id, true)
    } else if let Some(b) = chain_into(e_id, t_id) {
        (b, t_id, false)
    } else {
        return false;
    };
    if join == func.blocks[h].id {
        return false;
    }
    let Some(plan) = plan_arm(func, &blks, None, def_count) else {
        return false;
    };
    if plan.copies.is_empty() {
        return false;
    }
    let blocks: Vec<usize> = [h, join.0 as usize].into_iter().chain(blks).collect();
    if touched.any(&blocks, plan_cells(&plan)) {
        return false;
    }
    // every copy sets a cell the header already holds a value of; a
    // cell the header itself sets first counts
    let sets_in_h = |v: ValueId| func.blocks[h].insts.iter().any(|i| i.result == Some(v));
    if plan.copies.iter().any(|(v, _, _)| def_count(*v) < 2) {
        return false;
    }
    let preds = preds.get_or_insert_with(|| Preds::compute(func));
    if plan
        .copies
        .iter()
        .any(|(v, _, _)| !sets_in_h(*v) && !preds.set_on_every_path(func, *v, h))
    {
        return false;
    }
    // a copy reading another of the arm's cells would need the arm's
    // sequencing, which the selects do not keep
    let self_ref = plan.copies.iter().any(
        |(_, _, src)| matches!(src, Operand::Value(x) if plan.copies.iter().any(|(v, _, _)| v == x)),
    );
    if self_ref {
        return false;
    }
    // each cell loses its def in the arm and gains one in the header,
    // so its count stays exact: only the blocks change
    touched.mark(&blocks, []);
    let mut appended = plan.hoisted;
    stats.insts_hoisted += appended.len() as u32;
    for (vid, ty, src) in plan.copies {
        let picked = push_value(&mut func.values, ty);
        let (on_then, on_else) = if arm_is_then {
            (src, Operand::Value(vid))
        } else {
            (Operand::Value(vid), src)
        };
        appended.push(Inst {
            result: Some(picked),
            kind: InstKind::Select(ty, *cond, on_then, on_else),
            origin: None,
        });
        appended.push(Inst {
            result: Some(vid),
            kind: InstKind::Copy(ty, Operand::Value(picked)),
            origin: None,
        });
        stats.selects_formed += 1;
    }
    func.blocks[h].insts.extend(appended);
    func.blocks[h].term = Terminator::Br(join);
    for &b in &blocks[2..] {
        func.blocks[b].insts.clear();
    }
    stats.triangles_converted += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Type, ValueInfo};

    fn set(v: u32) -> Inst {
        Inst {
            result: Some(ValueId(v)),
            kind: InstKind::Copy(Type::I64, Operand::ConstI64(1)),
            origin: None,
        }
    }

    fn cond(then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::Value(ValueId(0)),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    /// b0 → b1 | b2; b1, b2 → b3; b3 → b4 | b3; `sets` says, per
    /// block, whether it sets %1.
    fn func(sets: [bool; 5]) -> Function {
        let terms = [
            cond(1, 2),
            Terminator::Br(BlockId(3)),
            Terminator::Br(BlockId(3)),
            cond(4, 3),
            Terminator::Ret(None),
        ];
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: terms
                .into_iter()
                .zip(sets)
                .enumerate()
                .map(|(i, (term, s))| Block {
                    id: BlockId(i as u32),
                    insts: if s { vec![set(1)] } else { vec![] },
                    term,
                })
                .collect(),
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                };
                2
            ],
            current_origin: None,
        }
    }

    fn holds(sets: [bool; 5], h: usize) -> bool {
        let f = func(sets);
        Preds::compute(&f).set_on_every_path(&f, ValueId(1), h)
    }

    #[test]
    fn both_arms_of_an_earlier_branch_setting_it_count() {
        assert!(holds([false, true, true, false, false], 3));
        assert!(holds([false, true, true, false, false], 4));
    }

    #[test]
    fn one_arm_setting_it_does_not() {
        assert!(!holds([false, true, false, false, false], 3));
    }

    #[test]
    fn the_entry_setting_it_covers_every_path() {
        assert!(holds([true, false, false, false, false], 3));
    }

    /// The loop's own block sets it only after the first entry.
    #[test]
    fn a_set_on_the_back_edge_alone_does_not() {
        assert!(!holds([false, false, false, true, false], 3));
        assert!(holds([false, false, false, true, false], 4));
    }
}
