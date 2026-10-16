//! Hot-successor chaining inside a loop's block range — the part of
//! LLVM MachineBlockPlacement that [`crate::block_layout`] left out.
//!
//! Codegen elides a `b` only when a branch target is the next block,
//! and flips a conditional's polarity only when one arm is. Lowering
//! order puts the arm it created first next, which is often the cold
//! one: a bounds check's grow call, a guard's kernel fallback, a slow
//! decode arm. Every such arm costs the hot path a taken branch per
//! iteration. Walking the loop from its header and placing, at each
//! step, the successor the path most likely continues into makes the
//! common path fall through.
//!
//! "Cold" is decided statically and conservatively: an edge that
//! leaves the loop, a block that ends the function (`ret` /
//! `unreachable`), a block that makes a call, or an arm the lowerer's
//! own guard marks rare (`torajs_core::ssa::branch_prob::cold_arms`:
//! the out-of-bounds side of a bounds check, a header-flag slow path,
//! a throw probe). Of two warm arms, the
//! one leading into a nested loop goes first; where nothing ranks the
//! arms, the one already placed first keeps its place, so such shapes
//! lay out exactly as before. (The opcode heuristic — equality is
//! usually false — was tried and dropped: collatz's `n % 2 === 0` is
//! true two times in three and the cell lost 14%.) Nested loops are
//! already contiguous when their parent is chained and move as one
//! unit, entered at their first block.

use torajs_core::ssa::{BlockId, Function, InstKind, Terminator};

mod block_set;
mod dom_view;
mod facts;
pub(crate) use block_set::BlockSet;
pub(crate) use dom_view::DomView;
pub(crate) use facts::{ChainFacts, ColdEdges};

/// Reorder `order[span]` (the loop's contiguous range) into
/// hot-successor chains starting at `header`. `children` are the bodies of the
/// loops nested in this one, each already contiguous. Answers whether
/// the order changed.
pub(crate) fn chain_loop_range(
    func: &Function,
    order: &mut [u32],
    span: std::ops::Range<usize>,
    header: u32,
    body: &BlockSet,
    children: &[(&BlockSet, u32)],
    facts: &ChainFacts,
) -> bool {
    let range = &order[span.clone()];
    let units = units_of(range, children);
    let unit = |u: usize| &range[units[u].clone()];
    // A block on every path into a nested loop (its entry, a preheader,
    // a rotation guard) leads toward the loop.
    let leads_to_loop = |b: u32| {
        children
            .iter()
            .any(|(_, h)| facts.dom.dominates(BlockId(b), BlockId(*h)))
    };
    // every block of the range sits in exactly one unit
    let mut unit_index = vec![usize::MAX; range.iter().max().map_or(0, |&m| m as usize + 1)];
    for (u, r) in units.iter().enumerate() {
        for &id in &range[r.clone()] {
            unit_index[id as usize] = u;
        }
    }
    let unit_of = |id: u32| (unit_index.get(id as usize).copied()).filter(|&u| u != usize::MAX);
    // where a chain ends, the next one starts at the first unplaced
    // warm unit: a unit every path to which runs a call (a grow, a
    // slow-path kernel, the exit epilogue) goes after every warm one,
    // in place order among themselves; taking units in place order
    // alone put such a call ahead of the warm blocks lowered after it
    // (a duplicated tail, an arm the lowerer emitted late), and the
    // allocator, which judges call crossing on a value's positional
    // hull, then saved every value those blocks read around the call;
    // units are judged once, when a chain first ends (most loops chain
    // whole and never ask)
    let cold = std::cell::OnceCell::new();
    let cold = |u: usize| {
        cold.get_or_init(|| {
            let firsts: Vec<u32> = units.iter().map(|r| range[r.start]).collect();
            facts.cold_paths(func, &|b| body.contains(b), &firsts)
        })[u]
    };
    let mut placed = Placed::new(units.len());
    let mut chain: Vec<usize> = Vec::with_capacity(units.len());
    let mut cur = unit_of(header).expect("header in its loop range");
    loop {
        placed.place(cur);
        chain.push(cur);
        let next = hot_successor(
            func,
            facts,
            unit(cur),
            body,
            &unit_of,
            &leads_to_loop,
            &placed.flags,
        )
        .or_else(|| placed.first_open(cold));
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    let new: Vec<u32> = chain
        .iter()
        .flat_map(|&u| unit(u).iter().copied())
        .collect();
    if new.as_slice() == range {
        return false;
    }
    order[span].copy_from_slice(&new);
    true
}

/// The range in units, in current order: a child loop's run of blocks,
/// or one block; each unit is its span of positions in `range`.
fn units_of(range: &[u32], children: &[(&BlockSet, u32)]) -> Vec<std::ops::Range<usize>> {
    let mut units: Vec<std::ops::Range<usize>> = Vec::new();
    // the child loop the last unit's first block belongs to
    let mut last_owner = None;
    for (i, &id) in range.iter().enumerate() {
        let owner = children.iter().position(|(c, _)| c.contains(id));
        match units.last_mut() {
            Some(u) if owner.is_some() && last_owner == owner => u.end = i + 1,
            _ => {
                units.push(i..i + 1);
                last_owner = owner;
            }
        }
    }
    units
}

/// The units placed so far. Placing only ever sets a flag, so the
/// first unplaced unit only moves forward; and since a unit's being
/// cold does not change, so does the first unplaced warm one.
struct Placed {
    flags: Vec<bool>,
    first_open: usize,
    first_warm: usize,
}

impl Placed {
    fn new(n: usize) -> Self {
        Placed {
            flags: vec![false; n],
            first_open: 0,
            first_warm: 0,
        }
    }

    fn place(&mut self, u: usize) {
        self.flags[u] = true;
        while self.first_open < self.flags.len() && self.flags[self.first_open] {
            self.first_open += 1;
        }
    }

    /// The first unplaced unit that is not cold, else the first
    /// unplaced one.
    fn first_open(&mut self, cold: impl Fn(usize) -> bool) -> Option<usize> {
        let n = self.flags.len();
        let flags = &self.flags;
        let skip = |u: usize| flags[u] || cold(u);
        while self.first_warm < n && skip(self.first_warm) {
            self.first_warm += 1;
        }
        (self.first_warm < n)
            .then_some(self.first_warm)
            .or_else(|| (self.first_open < n).then_some(self.first_open))
    }
}

/// The unplaced unit the path through `unit` most likely continues
/// into: the exits of its last block first (that is the branch a
/// fall-through can save), then those of its other blocks.
fn hot_successor(
    func: &Function,
    facts: &ChainFacts,
    unit: &[u32],
    body: &BlockSet,
    unit_of: &dyn Fn(u32) -> Option<usize>,
    leads_to_loop: &dyn Fn(u32) -> bool,
    placed: &[bool],
) -> Option<usize> {
    let last = *unit.last().expect("non-empty unit");
    let blocks = std::iter::once(last).chain(unit.iter().rev().skip(1).copied());
    for b in blocks {
        let targets = match &func.blocks[b as usize].term {
            Terminator::Br(t) => [Some(t.0), None],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => [Some(then_blk.0), Some(else_blk.0)],
            Terminator::Ret(_) | Terminator::Unreachable => [None, None],
        };
        let mut open = [0u32; 2];
        let mut n = 0;
        // the unit's own blocks are placed with it
        for t in targets.into_iter().flatten() {
            if body.contains(t) && unit_of(t).is_some_and(|u| !placed[u]) {
                open[n] = t;
                n += 1;
            }
        }
        let open = &open[..n];
        // Among warm arms, the one entering a nested loop first: the
        // loop-branch heuristic of static prediction (an edge into a
        // loop is taken far more often than the one bypassing it).
        let pick = open
            .iter()
            .copied()
            .filter(|t| !is_cold(func, *t) && !facts.cold_edges.contains(b, *t))
            .min_by_key(|t| (!leads_to_loop(*t), unit_of(*t)))
            .or_else(|| open.iter().copied().min_by_key(|t| unit_of(*t)));
        if let Some(t) = pick {
            return unit_of(t);
        }
    }
    None
}

/// A block the path rarely runs: one that ends the function, calls,
/// or drops an `any` (which may call).
fn is_cold(func: &Function, id: u32) -> bool {
    let block = &func.blocks[id as usize];
    matches!(block.term, Terminator::Ret(_) | Terminator::Unreachable)
        || block.insts.iter().any(|i| {
            matches!(
                i.kind,
                InstKind::Call(..) | InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::dominator::DominatorTree;
    use torajs_core::ssa::{FuncId, Inst, Operand, Type};

    /// b0 → b1(hdr: ? b2 : b6) ; b2: ? b3 : b4 ; b3 (call) → b7 ;
    /// b7 → b5 ; b4 → b5 ; b5 → b1 ; b6: ret
    #[test]
    fn a_block_only_a_call_reaches_is_cold() {
        let mut f = Function::new("f", Type::Void);
        let b: Vec<BlockId> = (0..8).map(|_| f.add_block()).collect();
        let c = f.add_param(Type::Bool, "c");
        let cond = |t: usize, e: usize| Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: b[t],
            else_blk: b[e],
        };
        f.set_term(b[0], Terminator::Br(b[1]));
        f.set_term(b[1], cond(2, 6));
        f.set_term(b[2], cond(3, 4));
        f.blocks[3].insts.push(Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), Vec::new()),
            origin: None,
        });
        f.set_term(b[3], Terminator::Br(b[7]));
        f.set_term(b[7], Terminator::Br(b[5]));
        f.set_term(b[4], Terminator::Br(b[5]));
        f.set_term(b[5], Terminator::Br(b[1]));
        f.set_term(b[6], Terminator::Ret(None));
        let dom = DominatorTree::compute(&f);
        let facts = ChainFacts::new(&f, DomView::whole(&dom));
        let body = BlockSet::of([1, 2, 3, 4, 5, 7].into_iter());
        let cold = |id: u32| facts.cold_path(&f, &|b| body.contains(b), id);
        assert!(cold(3) && cold(7), "the call and the block behind it");
        assert!(
            !cold(4) && !cold(5) && !cold(2),
            "the warm arm and the join"
        );
        // the shared walks answer what one walk per block does, in any
        // order of asking
        let all = [7, 5, 4, 3, 2, 1];
        let one_by_one: Vec<bool> = all.iter().map(|&b| cold(b)).collect();
        let in_body = |b| body.contains(b);
        assert_eq!(facts.cold_paths(&f, &in_body, &all), one_by_one);
    }

    /// A chain that ends goes on at the first unplaced warm unit; the
    /// cold ones come last, in place order.
    #[test]
    fn the_next_chain_starts_at_a_warm_unit() {
        let mut placed = Placed::new(5);
        let cold = |u: usize| [false, true, false, true, false][u];
        placed.place(0);
        assert_eq!(placed.first_open(cold), Some(2));
        placed.place(2);
        placed.place(4);
        assert_eq!(placed.first_open(cold), Some(1));
        placed.place(1);
        placed.place(3);
        assert_eq!(placed.first_open(cold), None);
    }
}
