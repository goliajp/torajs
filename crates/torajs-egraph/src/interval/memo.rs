//! What a round of the ascent can take from the rounds before it.
//!
//! Every round walks the whole function, yet past the first few only a
//! handful of cells still move. An instruction's value is a function of
//! the facts of the operands it reads, of the constraints in force
//! where it sits (the facts of the values they compare against, the
//! ranges of the merges among them), and, for an element load, of the
//! element point. When none of those moved since the instruction was
//! last evaluated, evaluating it again gives the value it gave then,
//! so the round reuses that value: the facts come out the same as when
//! every instruction is evaluated every round. Each move of a fact is
//! stamped with a counter that only grows, and an instruction is
//! evaluated again when something it reads carries a later stamp than
//! its own evaluation. A block nothing it reads moved in since it was
//! last walked is not walked again: its cells take the values its defs
//! gave then.

use torajs_core::ssa::{BlockId, Function, InstKind, ValueId, visit_value_operands};

use super::merge::Inherited;
use super::refine::Constraint;
use super::square::SquareBound;
use super::transfer::{AbsVal, NumFact};

/// The stamp of something that has not happened yet.
const NEVER: u32 = u32::MAX;

/// The next stamp; it never reaches [`NEVER`], which every check reads
/// as not yet.
fn tick(now: &mut u32) -> u32 {
    *now += 1;
    debug_assert!(*now != NEVER, "the move stamps ran out");
    *now
}

struct BlockMemo {
    /// its first instruction's index in the per-instruction tables
    first: u32,
    /// `Memo::values[bounds.0..bounds.1]`: the values its constraints
    /// compare against; `[bounds.0..inputs]`: those and every value
    /// its instructions read
    bounds: (u32, u32),
    inputs: u32,
    /// `Memo::cell_defs[cells.0..cells.1]`: its instructions that
    /// define a cell
    cells: (u32, u32),
    /// it loads a tracked element
    elem_reads: bool,
    /// the stamp it was last walked at
    walked_at: u32,
    /// the stamp of the latest move of one of its merges
    merges_at: u32,
}

pub(super) struct Memo {
    /// the stamp of the latest move
    now: u32,
    /// per value, the stamp of its latest move
    moved_at: Vec<u32>,
    blocks: Vec<BlockMemo>,
    values: Vec<ValueId>,
    cell_defs: Vec<u32>,
    /// per instruction, the stamp it was last evaluated at, and the value
    last_eval: Vec<u32>,
    last_ev: Vec<AbsVal>,
    /// per instruction, whether an evaluation of it read an operand's
    /// fact; one that never did has a value its kind alone fixes
    reads_facts: Vec<bool>,
    /// the merge ranges the previous round read
    merged: Vec<Option<NumFact>>,
    /// the stamp of the latest move of any element point
    elems_at: u32,
}

impl Memo {
    /// `cell`: whether a value is a cell; `elem_read`: whether an
    /// instruction's result loads a tracked element.
    pub(super) fn new(
        func: &Function,
        inherited: &Inherited,
        cell: impl Fn(ValueId) -> bool,
        elem_read: impl Fn(ValueId) -> bool,
    ) -> Self {
        let mut blocks = Vec::with_capacity(func.blocks.len());
        let mut values: Vec<ValueId> = Vec::new();
        let mut cell_defs: Vec<u32> = Vec::new();
        let mut n = 0u32;
        for (bi, block) in func.blocks.iter().enumerate() {
            let start = values.len();
            values.extend(
                inherited
                    .list(BlockId(bi as u32))
                    .iter()
                    .filter_map(|c| match c {
                        Constraint::Rel { bound, .. } => Some(*bound),
                        Constraint::Square {
                            bound: SquareBound::Value(b),
                            ..
                        } => Some(*b),
                        _ => None,
                    }),
            );
            let bounds_end = values.len();
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| values.push(v));
            }
            values[bounds_end..].sort_unstable_by_key(|v| v.0);
            let cells_start = cell_defs.len();
            let mut elem_reads = false;
            for (j, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    if cell(r) {
                        cell_defs.push(j as u32);
                    }
                    elem_reads |= elem_read(r);
                }
            }
            blocks.push(BlockMemo {
                first: n,
                bounds: (start as u32, bounds_end as u32),
                inputs: values.len() as u32,
                cells: (cells_start as u32, cell_defs.len() as u32),
                elem_reads,
                walked_at: NEVER,
                merges_at: 0,
            });
            n += block.insts.len() as u32;
        }
        let n = n as usize;
        Memo {
            now: 0,
            moved_at: vec![0; func.values.len()],
            blocks,
            values,
            cell_defs,
            last_eval: vec![NEVER; n],
            last_ev: vec![AbsVal::Bottom; n],
            reads_facts: vec![false; n],
            merged: Vec::new(),
            elems_at: 0,
        }
    }

    /// The fact of `v` moved.
    pub(super) fn moved(&mut self, v: ValueId) {
        self.moved_at[v.0 as usize] = tick(&mut self.now);
    }

    /// Some element point moved.
    pub(super) fn elems_moved(&mut self) {
        self.elems_at = tick(&mut self.now);
    }

    /// The round about to start reads the merge ranges `merged`: stamp
    /// every block one of whose merges reads differently than in the
    /// round before.
    pub(super) fn merges_read(&mut self, merged: &[Option<NumFact>], inherited: &Inherited) {
        if merged.is_empty() {
            return;
        }
        let first = self.merged.len() != merged.len();
        for (bi, b) in self.blocks.iter_mut().enumerate() {
            if inherited
                .slots(BlockId(bi as u32))
                .iter()
                .any(|&j| first || self.merged[j] != merged[j])
            {
                b.merges_at = tick(&mut self.now);
            }
        }
        self.merged.clear();
        self.merged.extend_from_slice(merged);
    }

    /// Nothing block `bi` reads moved since it was last walked: every
    /// instruction in it would give the value it gave then.
    pub(super) fn unmoved(&self, bi: usize) -> bool {
        let b = &self.blocks[bi];
        let at = b.walked_at;
        at != NEVER
            && b.merges_at <= at
            && !(b.elem_reads && self.elems_at > at)
            && self.values[b.bounds.0 as usize..b.inputs as usize]
                .iter()
                .all(|v| self.moved_at[v.0 as usize] <= at)
    }

    /// Block `bi` was walked.
    pub(super) fn walked(&mut self, bi: usize) {
        self.blocks[bi].walked_at = self.now;
    }

    /// Block `bi`'s instructions that define a cell, each with the
    /// value it was last evaluated to.
    pub(super) fn cell_defs(&self, bi: usize) -> impl Iterator<Item = (usize, AbsVal)> + '_ {
        let b = &self.blocks[bi];
        self.cell_defs[b.cells.0 as usize..b.cells.1 as usize]
            .iter()
            .map(move |&j| (j as usize, self.last_ev[(b.first + j) as usize]))
    }

    /// The stamp of the latest move of anything block `bi`'s
    /// constraints read.
    fn context(&self, bi: usize) -> u32 {
        let b = &self.blocks[bi];
        self.values[b.bounds.0 as usize..b.bounds.1 as usize]
            .iter()
            .map(|v| self.moved_at[v.0 as usize])
            .fold(b.merges_at, u32::max)
    }

    /// The value instruction `j` of block `bi` gave when last evaluated,
    /// if nothing it reads has moved since; `elem_read`: the
    /// instruction loads a tracked element.
    pub(super) fn reuse(
        &self,
        bi: usize,
        j: usize,
        kind: &InstKind,
        elem_read: bool,
    ) -> Option<AbsVal> {
        let i = self.blocks[bi].first as usize + j;
        let at = self.last_eval[i];
        if at == NEVER || (elem_read && self.elems_at > at) {
            return None;
        }
        if self.reads_facts[i] {
            if self.context(bi) > at {
                return None;
            }
            let mut stale = false;
            visit_value_operands(kind, |v| stale |= self.moved_at[v.0 as usize] > at);
            if stale {
                return None;
            }
        }
        Some(self.last_ev[i])
    }

    /// Instruction `j` of block `bi` was evaluated to `ev`, reading
    /// some operand's fact on the way when `read_facts`.
    pub(super) fn record(&mut self, bi: usize, j: usize, ev: AbsVal, read_facts: bool) {
        let i = self.blocks[bi].first as usize + j;
        self.last_eval[i] = self.now;
        self.last_ev[i] = ev;
        self.reads_facts[i] |= read_facts;
    }
}
