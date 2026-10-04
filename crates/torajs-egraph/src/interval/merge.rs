//! A cell read where paths meet. Each path into a block with several
//! predecessors may carry its own branch constraint on a cell (a loop
//! entered both from a faster copy of itself and from its own latch,
//! each under its own exit test), though no block dominating the merge
//! carries one. The cell's range at the merge is then the join of its
//! range at the end of every predecessor, each refined by what is known
//! there.
//!
//! The join stands at the merge's entry as a `Constraint::Merge`
//! placeholder, inherited like any edge constraint; each round reads it
//! as the range the previous round's facts give.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, Operand, ValueId};

use super::refine::Constraint;
use super::transfer::{AbsVal, NumFact};
use crate::branch_fold::point_eval::successors;

/// The merges worth a join: a block, a cell some constraint names at
/// the end of each of its predecessors, and those constraints per
/// predecessor.
pub(super) struct Merges {
    joins: Vec<Join>,
}

struct Join {
    block: BlockId,
    cell: ValueId,
    per_pred: Vec<Vec<Constraint>>,
}

impl Merges {
    /// `inherited` holds the constraints valid at each block's entry;
    /// one a block's own def of a value it names ends is not valid at
    /// the block's end.
    pub(super) fn collect(
        func: &Function,
        dom: &DominatorTree,
        inherited: &[Vec<Constraint>],
        block_defs: &[HashSet<ValueId>],
    ) -> Self {
        let mut joins = Vec::new();
        for (bi, ps) in reachable_preds(func, dom).iter().enumerate() {
            if ps.len() < 2 {
                continue;
            }
            let at_end: Vec<Vec<Constraint>> = ps
                .iter()
                .map(|&p| {
                    let defs = &block_defs[p];
                    inherited[p]
                        .iter()
                        .filter(|c| !c.killed_by(|v| defs.contains(&v)))
                        .copied()
                        .collect()
                })
                .collect();
            let mut cells: Vec<ValueId> = at_end[0]
                .iter()
                .map(Constraint::value)
                .filter(|v| block_defs.iter().any(|d| d.contains(v)))
                .filter(|v| {
                    at_end[1..]
                        .iter()
                        .all(|cs| cs.iter().any(|c| c.value() == *v))
                })
                .collect();
            cells.sort_by_key(|v| v.0);
            cells.dedup();
            for cell in cells {
                joins.push(Join {
                    block: BlockId(bi as u32),
                    cell,
                    per_pred: at_end.clone(),
                });
            }
        }
        Merges { joins }
    }

    /// The placeholder each merge puts at its block's entry.
    pub(super) fn placeholders(&self) -> impl Iterator<Item = (BlockId, Constraint)> + '_ {
        self.joins
            .iter()
            .map(|j| (j.block, Constraint::Merge(j.cell, j.block)))
    }

    /// Each merge's range under `state`: the join over its
    /// predecessors of the cell's range at their ends.
    pub(super) fn ranges(
        &self,
        state: &HashMap<ValueId, AbsVal>,
    ) -> HashMap<(BlockId, ValueId), NumFact> {
        let mut out = HashMap::new();
        for j in &self.joins {
            let mut joined: Option<NumFact> = None;
            for cs in &j.per_pred {
                let AbsVal::Fact(f) = super::lookup(&Operand::Value(j.cell), state, cs) else {
                    joined = None;
                    break;
                };
                joined = Some(joined.map_or(f, |g| g.join(f)));
            }
            if let Some(f) = joined {
                out.insert((j.block, j.cell), f);
            }
        }
        out
    }
}

/// Per block, the reachable blocks it is entered from.
fn reachable_preds(func: &Function, dom: &DominatorTree) -> Vec<Vec<usize>> {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); func.blocks.len()];
    for (bi, block) in func.blocks.iter().enumerate() {
        if !dom.is_reachable(block.id) {
            continue;
        }
        for s in successors(&block.term) {
            preds[s.0 as usize].push(bi);
        }
    }
    preds
}

/// `live` with every merge placeholder read as the range `ranges` gives
/// it, or dropped when it has none.
pub(super) fn resolve(
    live: &[Constraint],
    ranges: &HashMap<(BlockId, ValueId), NumFact>,
) -> Vec<Constraint> {
    live.iter()
        .filter_map(|c| match c {
            Constraint::Merge(v, b) => ranges.get(&(*b, *v)).map(|f| Constraint::Range(*v, *f)),
            other => Some(*other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    use super::super::{NumFact, analyze_function};

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cbr(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    #[test]
    fn a_cell_bounded_on_every_path_in_is_bounded_where_they_meet() {
        // bb0: %0 = copy 0 ; br bb1                       ; n
        // bb1: %1 = icmp slt %0, 100 ; cond_br %1, bb2, bb6
        // bb2: %2 = icmp eq %0, 50   ; cond_br %2, bb4, bb3
        // bb3: %3 = add %0, 1 ; %0 = copy %3 ; br bb1     ; fast latch
        // bb4: br bb5                                     ; to the slow copy
        // bb5: %4 = add %0, 1 ; %0 = copy %4 ; br bb7     ; from bb4 and bb8
        // bb6: ret
        // bb7: %5 = icmp slt %0, 100 ; cond_br %5, bb8, bb6
        // bb8: br bb5
        let copy = |r: u32, src: Operand| inst(r, InstKind::Copy(Type::I64, src));
        let add1 = |r: u32| inst(r, InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(1)));
        let lt100 = |r: u32| inst(r, InstKind::ICmp(IPred::Slt, v(0), Operand::ConstI64(100)));
        let br = |t: u32| Terminator::Br(BlockId(t));
        let eq50 = InstKind::ICmp(IPred::Eq, v(0), Operand::ConstI64(50));
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                block(0, vec![copy(0, Operand::ConstI64(0))], br(1)),
                block(1, vec![lt100(1)], cbr(1, 2, 6)),
                block(2, vec![inst(2, eq50)], cbr(2, 4, 3)),
                block(3, vec![add1(3), copy(0, v(3))], br(1)),
                block(4, vec![], br(5)),
                block(5, vec![add1(4), copy(0, v(4))], br(7)),
                block(6, vec![], Terminator::Ret(None)),
                block(7, vec![lt100(5)], cbr(5, 8, 6)),
                block(8, vec![], br(5)),
            ],
            values: [
                Type::I64,
                Type::Bool,
                Type::Bool,
                Type::I64,
                Type::I64,
                Type::Bool,
            ]
            .into_iter()
            .map(|ty| ValueInfo { ty, name: None })
            .collect(),
            current_origin: None,
        };
        let facts = analyze_function(&f);
        assert_eq!(facts.get(&ValueId(4)), Some(&NumFact::new(1, 100)));
        assert_eq!(facts.get(&ValueId(0)), Some(&NumFact::new(0, 100)));
    }
}
