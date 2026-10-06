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
//! gathered from a per-value index instead of copying every
//! constraint into every block it dominates: in long straight-line
//! code the full lists are blocks x constraints.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, ValueId, visit_value_operands};

use super::super::merge::{Inherited, Merges};
use super::super::square::SquareBound;
use super::{Constraint, Constraints};
use crate::branch_fold::point_eval::successors;

/// The merges, and each block's entry constraints with the merge
/// placeholders among them. A list holds, in the order a full list
/// would, every constraint naming a value the block reads or a value
/// a square bound reads; nothing a lookup in the block reads is
/// missing from it.
pub(in super::super) fn inherited(
    func: &Function,
    dom: &DominatorTree,
    mut constraints: Constraints,
    block_defs: &[HashSet<ValueId>],
) -> (Merges, Inherited) {
    let watched = square_values(func, &constraints);
    let merges = {
        let mut scope = Scope::new(func, dom, &constraints, block_defs);
        let mut cells: Vec<ValueId> = block_defs.iter().flatten().copied().collect();
        cells.extend_from_slice(&watched);
        cells.sort_by_key(|v| v.0);
        cells.dedup();
        Merges::collect(
            func,
            dom,
            |p| scope.holding(BlockId(p as u32), &cells),
            block_defs,
        )
    };
    for (b, c) in merges.placeholders() {
        constraints.add(b, c);
    }
    let mut scope = Scope::new(func, dom, &constraints, block_defs);
    let mut values: Vec<ValueId> = Vec::new();
    let lists = func
        .blocks
        .iter()
        .enumerate()
        .map(|(bi, block)| {
            values.clear();
            values.extend_from_slice(&watched);
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| values.push(v));
            }
            values.sort_by_key(|v| v.0);
            values.dedup();
            scope.holding(BlockId(bi as u32), &values)
        })
        .collect();
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
    /// blocks `a` dominates that define a value the constraint names
    killers: Vec<BlockId>,
}

struct Scope<'a> {
    func: &'a Function,
    dom: &'a DominatorTree,
    entries: Vec<Entry>,
    by_value: HashMap<ValueId, Vec<u32>>,
    /// per entry with killers, the blocks it does not hold in
    dead: HashMap<u32, Vec<u64>>,
}

impl<'a> Scope<'a> {
    fn new(
        func: &'a Function,
        dom: &'a DominatorTree,
        constraints: &Constraints,
        block_defs: &[HashSet<ValueId>],
    ) -> Self {
        let def_blocks = def_blocks(block_defs);
        let mut entries = Vec::new();
        let mut by_value: HashMap<ValueId, Vec<u32>> = HashMap::default();
        for (rank, &a) in dom.reverse_postorder().iter().enumerate() {
            for (pos, c) in constraints.at(a).iter().enumerate() {
                let killers = killers(c, a, dom, &def_blocks);
                by_value
                    .entry(c.value())
                    .or_default()
                    .push(entries.len() as u32);
                entries.push(Entry {
                    a,
                    rank: rank as u32,
                    pos: pos as u32,
                    c: *c,
                    killers,
                });
            }
        }
        Scope {
            func,
            dom,
            entries,
            by_value,
            dead: HashMap::default(),
        }
    }

    /// The constraints naming one of `values` (sorted, deduplicated)
    /// that hold at `b`'s entry, nearest edge first.
    fn holding(&mut self, b: BlockId, values: &[ValueId]) -> Vec<Constraint> {
        let mut ids: Vec<u32> = Vec::new();
        for v in values {
            let Some(vs) = self.by_value.get(v) else {
                continue;
            };
            for &id in vs {
                if self.dom.dominates(self.entries[id as usize].a, b) {
                    ids.push(id);
                }
            }
        }
        ids.retain(|&id| !self.is_dead(id, b));
        let key = |id: &u32| {
            let e = &self.entries[*id as usize];
            (std::cmp::Reverse(e.rank), e.pos)
        };
        ids.sort_unstable_by_key(key);
        ids.iter().map(|&id| self.entries[id as usize].c).collect()
    }

    fn is_dead(&mut self, id: u32, b: BlockId) -> bool {
        let e = &self.entries[id as usize];
        if e.killers.is_empty() {
            return false;
        }
        let bits = self
            .dead
            .entry(id)
            .or_insert_with(|| reached(self.func, e.a, &e.killers));
        let bi = b.0 as usize;
        bits[bi / 64] >> (bi % 64) & 1 == 1
    }
}

/// The blocks defining each cell.
fn def_blocks(block_defs: &[HashSet<ValueId>]) -> HashMap<ValueId, Vec<BlockId>> {
    let mut out: HashMap<ValueId, Vec<BlockId>> = HashMap::default();
    for (bi, defs) in block_defs.iter().enumerate() {
        for v in defs {
            out.entry(*v).or_default().push(BlockId(bi as u32));
        }
    }
    out
}

/// The blocks `a` dominates whose def ends `c` (see
/// [`Constraint::killed_by`]).
fn killers(
    c: &Constraint,
    a: BlockId,
    dom: &DominatorTree,
    def_blocks: &HashMap<ValueId, Vec<BlockId>>,
) -> Vec<BlockId> {
    let bound = match c {
        Constraint::Square {
            bound: SquareBound::Value(b),
            ..
        } => Some(*b),
        _ => None,
    };
    [Some(c.value()), bound]
        .into_iter()
        .flatten()
        .filter_map(|v| def_blocks.get(&v))
        .flatten()
        .copied()
        .filter(|&x| dom.dominates(a, x))
        .collect()
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
