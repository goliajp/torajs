//! Which entry constraints hold at a block. One set at `a`'s entry
//! holds in a block `b` that `a` dominates unless a value it names
//! (`block_defs`: the cells each block defines) is defined on some
//! path from `a` to `b` that does not pass `a` again (passing `a` sets
//! it anew): in `a` itself, in any block between, or in `b` when a
//! cycle avoiding `a` leads back to `b`. `b`'s defs on the way in sit
//! after its entry: block-local evaluation kills at the def, so a tail
//! `i = i + 1` still reads its loop bound at the increment.
//!
//! A block's list is only ever read for the values the block itself
//! reads (plus the square bounds, read everywhere), so each list is
//! gathered per value instead of copying every constraint into every
//! block it dominates: in long straight-line code the full lists are
//! blocks x constraints. The lists come out of one preorder walk of the
//! dominator tree that keeps, per value, the constraints set on the
//! path from the entry (the scoped tables of SSA renaming): a block
//! reads exactly the ones whose block dominates it, never testing the
//! rest — a matcher comparing one character in thousands of blocks
//! would otherwise test every compare against every block.

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, ValueId, visit_value_operands};

use super::super::merge::{Inherited, Merges, reachable_preds};
use super::super::square::SquareBound;
use super::{Constraint, Constraints};
use crate::throw_check_fold::successors;

/// No entry, in the per-value stacks and the per-block lists.
const NONE: u32 = u32::MAX;

/// The merges, and each block's entry constraints with the merge
/// placeholders among them. A list holds, in the order a full list
/// would, every constraint naming a value the block reads or a value
/// a square bound reads; nothing a lookup in the block reads is
/// missing from it.
pub(in super::super) fn inherited(
    func: &Function,
    dom: &DominatorTree,
    mut constraints: Constraints,
    block_defs: &[Vec<ValueId>],
) -> (Merges, Inherited) {
    let n = func.blocks.len();
    if (0..n).all(|b| constraints.at(BlockId(b as u32)).is_empty()) {
        let merges = Merges::none();
        let inherited = merges.inherited(vec![Vec::new(); n]);
        return (merges, inherited);
    }
    let watched = square_values(func, &constraints);
    let mut scope = Scope::new(func, dom, &constraints, block_defs);
    let preds = reachable_preds(func, dom);
    let merges = {
        let mut cells: Vec<ValueId> = block_defs.iter().flatten().copied().collect();
        cells.extend_from_slice(&watched);
        let mut entered_from = vec![false; n];
        for ps in preds.per_block().filter(|ps| ps.len() >= 2) {
            for &p in ps {
                entered_from[p] = true;
            }
        }
        let at_pred = scope.holding_each(|b, values| {
            if entered_from[b] {
                values.extend_from_slice(&cells);
            }
            entered_from[b]
        });
        Merges::collect(&preds, |p| &at_pred[p], block_defs, func.values.len())
    };
    for (b, c) in merges.placeholders() {
        constraints.add(b, c);
        scope.push(b, constraints.at(b).len() - 1, c);
    }
    let lists = scope.holding_each(|bi, values| {
        values.extend_from_slice(&watched);
        for inst in &func.blocks[bi].insts {
            visit_value_operands(&inst.kind, |v| values.push(v));
        }
        true
    });
    let inherited = merges.inherited(lists);
    (merges, inherited)
}

/// The values a square constraint, or the bound it is compared
/// against, names: a square's bound is read wherever it is live.
fn square_values(func: &Function, constraints: &Constraints) -> Vec<ValueId> {
    let mut out = Vec::new();
    for b in 0..func.blocks.len() {
        for c in constraints.at(BlockId(b as u32)) {
            if let Constraint::Square { value, bound, .. } = c {
                out.push(*value);
                if let SquareBound::Value(v) = bound {
                    out.push(*v);
                }
            }
        }
    }
    out.sort_by_key(|v| v.0);
    out.dedup();
    out
}

struct Entry {
    /// the block whose entry sets it, and its rank in reverse postorder
    a: BlockId,
    rank: u32,
    pos: u32,
    c: Constraint,
    /// `Scope::killers[killers.0..killers.1]`: the blocks `a` dominates
    /// that define a value the constraint names
    killers: (u32, u32),
    /// the next entry of the same block, in position order
    next: u32,
    /// the entry of the same value below this one on the walk's path
    below: u32,
}

struct Scope<'a> {
    func: &'a Function,
    dom: &'a DominatorTree,
    /// `(cell, block defining it)`, sorted
    def_blocks: Vec<(ValueId, BlockId)>,
    /// each reachable block's rank in reverse postorder
    rank: Vec<u32>,
    entries: Vec<Entry>,
    killers: Vec<BlockId>,
    /// per block, its first and last entry
    first: Vec<u32>,
    last: Vec<u32>,
    /// per entry with killers, once asked, the blocks it does not hold in
    dead: Vec<Option<Vec<u64>>>,
}

impl<'a> Scope<'a> {
    fn new(
        func: &'a Function,
        dom: &'a DominatorTree,
        constraints: &Constraints,
        block_defs: &[Vec<ValueId>],
    ) -> Self {
        let mut def_blocks: Vec<(ValueId, BlockId)> = block_defs
            .iter()
            .enumerate()
            .flat_map(|(bi, ds)| ds.iter().map(move |v| (*v, BlockId(bi as u32))))
            .collect();
        def_blocks.sort_unstable_by_key(|(v, b)| (v.0, b.0));
        let n = func.blocks.len();
        // room for the merge placeholders added later
        let room = (0..n)
            .map(|b| constraints.at(BlockId(b as u32)).len())
            .sum::<usize>()
            * 2;
        let mut scope = Scope {
            func,
            dom,
            def_blocks,
            rank: vec![NONE; n],
            entries: Vec::with_capacity(room),
            killers: Vec::with_capacity(room),
            first: vec![NONE; n],
            last: vec![NONE; n],
            dead: Vec::with_capacity(room),
        };
        for (rank, &a) in dom.reverse_postorder().iter().enumerate() {
            scope.rank[a.0 as usize] = rank as u32;
            for (pos, c) in constraints.at(a).iter().enumerate() {
                scope.push(a, pos, *c);
            }
        }
        scope
    }

    /// Record `c`, set at reachable block `a`'s entry at position `pos`.
    fn push(&mut self, a: BlockId, pos: usize, c: Constraint) {
        let id = self.entries.len() as u32;
        let start = self.killers.len() as u32;
        let bound = match c {
            Constraint::Square {
                bound: SquareBound::Value(b),
                ..
            } => Some(b),
            _ => None,
        };
        for v in [Some(c.value()), bound].into_iter().flatten() {
            let from = self.def_blocks.partition_point(|(d, _)| d.0 < v.0);
            for &(d, x) in &self.def_blocks[from..] {
                if d != v {
                    break;
                }
                if self.dom.dominates(a, x) {
                    self.killers.push(x);
                }
            }
        }
        let ai = a.0 as usize;
        match self.last[ai] {
            NONE => self.first[ai] = id,
            prev => self.entries[prev as usize].next = id,
        }
        self.last[ai] = id;
        self.entries.push(Entry {
            a,
            rank: self.rank[ai],
            pos: pos as u32,
            c,
            killers: (start, self.killers.len() as u32),
            next: NONE,
            below: NONE,
        });
        self.dead.push(None);
    }

    /// For each reachable block `b` that `values` fills a list of
    /// values for (returning true), the constraints naming one of them
    /// that hold at `b`'s entry, nearest edge first; an empty list for
    /// every other block.
    fn holding_each(
        &mut self,
        mut values: impl FnMut(usize, &mut Vec<ValueId>) -> bool,
    ) -> Vec<Vec<Constraint>> {
        let mut out = vec![Vec::new(); self.func.blocks.len()];
        let Some(&root) = self.dom.reverse_postorder().first() else {
            return out;
        };
        let nv = self.func.values.len();
        // per value, the top of its stack of entries set on the path
        // from the entry to the block being visited
        let mut top = vec![NONE; nv];
        // the entries pushed on the path, in push order
        let mut trail: Vec<u32> = Vec::with_capacity(self.entries.len());
        // `seen[v] == epoch`: `v` was already gathered for this block
        let mut seen = vec![0u32; nv];
        let mut wanted: Vec<ValueId> = Vec::with_capacity(64);
        let mut ids: Vec<u32> = Vec::with_capacity(self.entries.len());
        // (block, next child to visit, trail length before its entries)
        let mut walk: Vec<(BlockId, usize, usize)> = vec![(root, 0, 0)];
        self.enter(root, &mut top, &mut trail);
        let mut epoch = 0u32;
        while let Some(&mut (b, ref mut next, mark)) = walk.last_mut() {
            if *next == 0 {
                wanted.clear();
                if values(b.0 as usize, &mut wanted) {
                    epoch += 1;
                    ids.clear();
                    for v in &wanted {
                        let vi = v.0 as usize;
                        if seen[vi] == epoch {
                            continue;
                        }
                        seen[vi] = epoch;
                        let mut id = top[vi];
                        while id != NONE {
                            ids.push(id);
                            id = self.entries[id as usize].below;
                        }
                    }
                    ids.retain(|&id| !self.is_dead(id, b));
                    let key = |id: &u32| {
                        let e = &self.entries[*id as usize];
                        (std::cmp::Reverse(e.rank), e.pos)
                    };
                    ids.sort_unstable_by_key(key);
                    out[b.0 as usize] = ids.iter().map(|&id| self.entries[id as usize].c).collect();
                }
            }
            match self.dom.children(b).get(*next) {
                Some(&child) => {
                    *next += 1;
                    let mark = trail.len();
                    self.enter(child, &mut top, &mut trail);
                    walk.push((child, 0, mark));
                }
                None => {
                    while trail.len() > mark {
                        let id = trail.pop().expect("an entry above the mark");
                        let e = &self.entries[id as usize];
                        top[e.c.value().0 as usize] = e.below;
                    }
                    walk.pop();
                }
            }
        }
        out
    }

    /// Push the entries block `b` sets onto the stacks of their values.
    fn enter(&mut self, b: BlockId, top: &mut [u32], trail: &mut Vec<u32>) {
        let mut id = self.first[b.0 as usize];
        while id != NONE {
            let e = &mut self.entries[id as usize];
            let v = e.c.value().0 as usize;
            e.below = top[v];
            top[v] = id;
            trail.push(id);
            id = e.next;
        }
    }

    fn is_dead(&mut self, id: u32, b: BlockId) -> bool {
        let e = &self.entries[id as usize];
        if e.killers.0 == e.killers.1 {
            return false;
        }
        let bits = self.dead[id as usize].get_or_insert_with(|| {
            let killers = &self.killers[e.killers.0 as usize..e.killers.1 as usize];
            reached(self.func, e.a, killers)
        });
        let bi = b.0 as usize;
        bits[bi / 64] >> (bi % 64) & 1 == 1
    }
}

/// The blocks reached from the successors of `killers` without
/// passing `a`, as a bit per block.
fn reached(func: &Function, a: BlockId, killers: &[BlockId]) -> Vec<u64> {
    let mut bits = vec![0u64; func.blocks.len().div_ceil(64)];
    let mut work: Vec<BlockId> = killers
        .iter()
        .flat_map(|x| successors(&func.blocks[x.0 as usize].term))
        .collect();
    while let Some(s) = work.pop() {
        let si = s.0 as usize;
        if s == a || bits[si / 64] >> (si % 64) & 1 == 1 {
            continue;
        }
        bits[si / 64] |= 1 << (si % 64);
        work.extend(successors(&func.blocks[si].term));
    }
    bits
}
