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

use crate::id_hash::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, Operand, ValueId};

use super::refine::Constraint;
use super::transfer::{AbsVal, NumFact};
use crate::throw_check_fold::successors;

/// The merges worth a join: a block, a cell some constraint names at
/// the end of each of its predecessors, and those constraints per
/// predecessor.
pub(super) struct Merges {
    joins: Vec<Join>,
}

struct Join {
    block: BlockId,
    cell: ValueId,
    /// per predecessor, what [`super::lookup`] of the cell reads from
    /// its constraints there (see [`reads_of`])
    per_pred: Vec<Vec<Constraint>>,
}

impl Merges {
    /// No merge: what [`collect`](Self::collect) finds where no block
    /// carries a constraint.
    pub(super) fn none() -> Self {
        Merges { joins: Vec::new() }
    }

    /// `preds` is [`reachable_preds`]. `inherited` gives, for a block
    /// a merge is entered from, the constraints valid at its entry that
    /// name a cell or a value a square bound reads; one a block's own
    /// def of a value it names ends is not valid at the block's end.
    /// `values` is the function's value count.
    pub(super) fn collect<'c>(
        preds: &Preds,
        inherited: impl Fn(usize) -> &'c [Constraint],
        block_defs: &[Vec<ValueId>],
        values: usize,
    ) -> Self {
        // every value some block defines: a cell, not a value fixed at its def
        let mut defined = vec![false; values];
        for v in block_defs.iter().flatten() {
            defined[v.0 as usize] = true;
        }
        let at_end = |p: usize| {
            let defs = &block_defs[p];
            inherited(p)
                .iter()
                .filter(move |c| !c.killed_by(|v| defs.contains(&v)))
        };
        // per value, how many of a merge's predecessors name it at their
        // end, and the predecessor that counted it last
        let mut named = vec![0usize; values];
        let mut counted = vec![usize::MAX; values];
        let mut turn = 0;
        let mut joins = Vec::new();
        for (bi, ps) in preds.per_block().enumerate() {
            if ps.len() < 2 {
                continue;
            }
            for &p in ps {
                turn += 1;
                for c in at_end(p) {
                    let v = c.value().0 as usize;
                    if counted[v] != turn {
                        counted[v] = turn;
                        named[v] += 1;
                    }
                }
            }
            let mut cells: Vec<ValueId> = at_end(ps[0])
                .map(Constraint::value)
                .filter(|v| defined[v.0 as usize] && named[v.0 as usize] == ps.len())
                .collect();
            for &p in ps {
                for c in at_end(p) {
                    named[c.value().0 as usize] = 0;
                }
            }
            cells.sort_by_key(|v| v.0);
            cells.dedup();
            if cells.is_empty() {
                continue;
            }
            let at_end: Vec<Vec<Constraint>> =
                ps.iter().map(|&p| at_end(p).copied().collect()).collect();
            for cell in cells {
                joins.push(Join {
                    block: BlockId(bi as u32),
                    cell,
                    per_pred: at_end.iter().map(|cs| reads_of(cs, cell)).collect(),
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

    /// Each block's inherited constraints (placeholders included), with
    /// the join each placeholder among them stands for.
    pub(super) fn inherited(&self, lists: Vec<Vec<Constraint>>) -> Inherited {
        let index: HashMap<(BlockId, ValueId), usize> = self
            .joins
            .iter()
            .enumerate()
            .map(|(i, j)| ((j.block, j.cell), i))
            .collect();
        let slots = lists
            .iter()
            .map(|cs| {
                cs.iter()
                    .filter_map(|c| match c {
                        Constraint::Merge(v, b) => Some(index[&(*b, *v)]),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        Inherited { lists, slots }
    }

    /// Each merge's range under `state`, indexed like the joins: the
    /// join over its predecessors of the cell's range at their ends.
    pub(super) fn ranges(&self, state: &super::State) -> Vec<Option<NumFact>> {
        let mut out = vec![None; self.joins.len()];
        for (i, j) in self.joins.iter().enumerate() {
            let mut joined: Option<NumFact> = None;
            for cs in j.per_pred.iter() {
                let AbsVal::Fact(f) = super::lookup(&Operand::Value(j.cell), state, cs) else {
                    joined = None;
                    break;
                };
                joined = Some(joined.map_or(f, |g| g.join(f)));
            }
            out[i] = joined;
        }
        out
    }
}

/// The part of `cs` a lookup of `cell` reads: the constraints that
/// speak of the cell, in order. A square bound reads the rest of the
/// list for its bound, so with one present the whole list stays.
fn reads_of(cs: &[Constraint], cell: ValueId) -> Vec<Constraint> {
    let own = |c: &&Constraint| {
        matches!(
            c,
            Constraint::Range(..)
                | Constraint::NonZero(_)
                | Constraint::Rel { .. }
                | Constraint::Square { .. }
        ) && c.value() == cell
    };
    if cs
        .iter()
        .filter(own)
        .any(|c| matches!(c, Constraint::Square { .. }))
    {
        return cs.to_vec();
    }
    cs.iter().filter(own).copied().collect()
}

/// Per block, the reachable blocks it is entered from, one edge each.
pub(super) struct Preds {
    /// block `b`'s are `from[start[b]..start[b + 1]]`, in block order
    start: Vec<u32>,
    from: Vec<usize>,
}

impl Preds {
    /// Each block's predecessors, in block order.
    pub(super) fn per_block(&self) -> impl Iterator<Item = &[usize]> {
        self.start
            .windows(2)
            .map(|w| &self.from[w[0] as usize..w[1] as usize])
    }
}

pub(super) fn reachable_preds(func: &Function, dom: &DominatorTree) -> Preds {
    let n = func.blocks.len();
    let reachable = |b: &torajs_core::ssa::Block| dom.is_reachable(b.id);
    let mut start = vec![0u32; n + 1];
    for block in func.blocks.iter().filter(|b| reachable(b)) {
        for s in successors(&block.term) {
            start[s.0 as usize + 1] += 1;
        }
    }
    for b in 0..n {
        start[b + 1] += start[b];
    }
    let mut fill = start.clone();
    let mut from = vec![0usize; start[n] as usize];
    for (bi, block) in func.blocks.iter().enumerate() {
        if !reachable(block) {
            continue;
        }
        for s in successors(&block.term) {
            let at = &mut fill[s.0 as usize];
            from[*at as usize] = bi;
            *at += 1;
        }
    }
    Preds { start, from }
}

/// The constraints valid at each block's entry; `slots[b]` names the
/// join of each merge placeholder in `lists[b]`, in order.
pub(super) struct Inherited {
    lists: Vec<Vec<Constraint>>,
    slots: Vec<Vec<usize>>,
}

impl Inherited {
    /// Block `b`'s constraints as stored, placeholders unresolved.
    pub(super) fn list(&self, b: BlockId) -> &[Constraint] {
        &self.lists[b.0 as usize]
    }

    /// Whether block `b`'s list holds a merge placeholder.
    pub(super) fn has_merges(&self, b: BlockId) -> bool {
        !self.slots[b.0 as usize].is_empty()
    }

    /// The joins block `b`'s placeholders stand for.
    pub(super) fn slots(&self, b: BlockId) -> &[usize] {
        &self.slots[b.0 as usize]
    }

    /// Block `b`'s constraints with every merge placeholder read as the
    /// range `ranges` gives its join, or dropped when it has none.
    /// The list is written into `out` (cleared first), so one buffer
    /// serves every block of a round.
    pub(super) fn resolve(
        &self,
        b: BlockId,
        ranges: &[Option<NumFact>],
        out: &mut Vec<Constraint>,
    ) {
        let live = &self.lists[b.0 as usize];
        out.clear();
        let mut slot = self.slots[b.0 as usize].iter();
        for c in live {
            match c {
                Constraint::Merge(v, _) => {
                    let j = *slot.next().expect("a slot per placeholder");
                    if let Some(f) = ranges[j] {
                        out.push(Constraint::Range(*v, f));
                    }
                }
                other => out.push(*other),
            }
        }
    }
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
