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
//! gave then. A move stamps the blocks that read the value as it
//! happens, so asking whether a block has to be walked costs one
//! comparison whatever the block reads; a merge range none of whose
//! reads moved is likewise kept from the round before.

use torajs_core::ssa::{BlockId, Function, InstKind, ValueId, visit_value_operands};

use super::merge::{Inherited, Merges};
use super::refine::Constraint;
use super::square::SquareBound;
use super::state::State;
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
    /// the stamp of the latest move of a value its constraints compare
    /// against
    bounds_at: u32,
    /// the stamp of the latest move of one of those or of a value its
    /// instructions read
    inputs_at: u32,
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
    /// value `v`'s readers are `readers[reader_start[v]..reader_start[v + 1]]`
    reader_start: Vec<u32>,
    readers: Vec<Reader>,
    blocks: Vec<BlockMemo>,
    /// per merge, the stamp of the latest move of a value its range reads
    joins_at: Vec<u32>,
    cell_defs: Vec<u32>,
    /// per instruction, the stamp it was last evaluated at, and the value
    last_eval: Vec<u32>,
    last_ev: Vec<AbsVal>,
    /// per instruction, whether an evaluation of it read an operand's
    /// fact; one that never did has a value its kind alone fixes
    reads_facts: Vec<bool>,
    /// the stamp the merge ranges were last read at
    merged_at: u32,
    /// per merge, whether the latest read changed its range
    merge_moved: Vec<bool>,
    /// the stamp of the latest move of any element point
    elems_at: u32,
}

/// What reads a value: a block, through an instruction operand or
/// through a constraint that compares against it (`bound`), or, past
/// the last block, a merge's range.
#[derive(Clone, Copy)]
struct Reader {
    target: u32,
    bound: bool,
}

impl Memo {
    /// `cell`: whether a value is a cell; `elem_read`: whether an
    /// instruction's result loads a tracked element.
    pub(super) fn new(
        func: &Function,
        inherited: &Inherited,
        merges: &Merges,
        cell: impl Fn(ValueId) -> bool,
        elem_read: impl Fn(ValueId) -> bool,
    ) -> Self {
        let mut blocks = Vec::with_capacity(func.blocks.len());
        // (value, reader, through a constraint bound), reader by reader
        let mut reads: Vec<(u32, u32, bool)> = Vec::new();
        let mut cell_defs: Vec<u32> = Vec::new();
        let mut n = 0u32;
        for (bi, block) in func.blocks.iter().enumerate() {
            let b = bi as u32;
            reads.extend(
                inherited
                    .list(BlockId(b))
                    .iter()
                    .filter_map(|c| match c {
                        Constraint::Rel { bound, .. } => Some(*bound),
                        Constraint::Square {
                            bound: SquareBound::Value(v),
                            ..
                        } => Some(*v),
                        _ => None,
                    })
                    .map(|v| (v.0, b, true)),
            );
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| reads.push((v.0, b, false)));
            }
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
                bounds_at: 0,
                inputs_at: 0,
                cells: (cells_start as u32, cell_defs.len() as u32),
                elem_reads,
                walked_at: NEVER,
                merges_at: 0,
            });
            n += block.insts.len() as u32;
        }
        let n = n as usize;
        let nb = func.blocks.len() as u32;
        for j in 0..merges.len() {
            merges.visit_reads(j, |v| reads.push((v.0, nb + j as u32, false)));
        }
        let (reader_start, readers) = index_readers(&reads, func.values.len());
        Memo {
            now: 0,
            moved_at: vec![0; func.values.len()],
            reader_start,
            readers,
            blocks,
            joins_at: vec![0; merges.len()],
            cell_defs,
            last_eval: vec![NEVER; n],
            last_ev: vec![AbsVal::Bottom; n],
            reads_facts: vec![false; n],
            merged_at: 0,
            merge_moved: Vec::new(),
            elems_at: 0,
        }
    }

    /// The fact of `v` moved.
    pub(super) fn moved(&mut self, v: ValueId) {
        let at = tick(&mut self.now);
        let i = v.0 as usize;
        self.moved_at[i] = at;
        let span = self.reader_start[i] as usize..self.reader_start[i + 1] as usize;
        for r in &self.readers[span] {
            let Some(b) = self.blocks.get_mut(r.target as usize) else {
                self.joins_at[r.target as usize - self.blocks.len()] = at;
                continue;
            };
            b.inputs_at = at;
            if r.bound {
                b.bounds_at = at;
            }
        }
    }

    /// Some element point moved.
    pub(super) fn elems_moved(&mut self) {
        self.elems_at = tick(&mut self.now);
    }

    /// The round about to start reads the merge ranges: bring `merged`
    /// (indexed like the joins of `merges`) up to `state`, and stamp
    /// every block one of whose merges reads differently than in the
    /// round before. A join none of whose reads moved since the last
    /// read gives the range it gave then, so only the others are
    /// computed again.
    pub(super) fn read_merges(
        &mut self,
        merges: &Merges,
        state: &State,
        inherited: &Inherited,
        merged: &mut Vec<Option<NumFact>>,
    ) {
        if merges.len() == 0 {
            return;
        }
        let first = merged.len() != merges.len();
        if first {
            merged.clear();
            merged.extend((0..merges.len()).map(|j| merges.range(j, state)));
        } else {
            self.merge_moved.clear();
            let mut any = false;
            for (j, old) in merged.iter_mut().enumerate() {
                let new = if self.joins_at[j] > self.merged_at {
                    merges.range(j, state)
                } else {
                    *old
                };
                any |= new != *old;
                self.merge_moved.push(new != *old);
                *old = new;
            }
            if !any {
                self.merged_at = self.now;
                return;
            }
        }
        for (bi, b) in self.blocks.iter_mut().enumerate() {
            if inherited
                .slots(BlockId(bi as u32))
                .iter()
                .any(|&j| first || self.merge_moved[j])
            {
                b.merges_at = tick(&mut self.now);
            }
        }
        self.merged_at = self.now;
    }

    /// Nothing block `bi` reads moved since it was last walked: every
    /// instruction in it would give the value it gave then.
    pub(super) fn unmoved(&self, bi: usize) -> bool {
        let b = &self.blocks[bi];
        let at = b.walked_at;
        at != NEVER
            && b.merges_at <= at
            && !(b.elem_reads && self.elems_at > at)
            && b.inputs_at <= at
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
        b.bounds_at.max(b.merges_at)
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

/// The readers of every value as one table: value `v`'s are
/// `readers[start[v]..start[v + 1]]`, each reader once, flagged `bound`
/// when it compares against `v`. `reads` comes reader by reader, so a
/// reader's repeated reads of a value follow its first one with no
/// other reader in between.
fn index_readers(reads: &[(u32, u32, bool)], values: usize) -> (Vec<u32>, Vec<Reader>) {
    let mut last = vec![u32::MAX; values];
    let mut start = vec![0u32; values + 1];
    for &(v, target, _) in reads {
        if last[v as usize] != target {
            last[v as usize] = target;
            start[v as usize + 1] += 1;
        }
    }
    for i in 0..values {
        start[i + 1] += start[i];
    }
    let none = Reader {
        target: 0,
        bound: false,
    };
    let mut readers = vec![none; start[values] as usize];
    // `start[v]` serves as the fill cursor of `v` and ends where `v + 1`
    // starts; shifted back below
    last.fill(u32::MAX);
    for &(v, target, bound) in reads {
        let i = v as usize;
        if last[i] != target {
            last[i] = target;
            readers[start[i] as usize] = Reader { target, bound };
            start[i] += 1;
        } else {
            readers[start[i] as usize - 1].bound |= bound;
        }
    }
    start.copy_within(0..values, 1);
    start[0] = 0;
    (start, readers)
}

#[cfg(test)]
mod tests {
    use super::index_readers;

    #[test]
    fn each_reader_is_listed_once_per_value_with_its_bound_flag() {
        // value 1: block 0 as an operand and as a bound, then block 2;
        // value 2: block 0, then reader 3 twice; value 0: none
        let reads = [
            (1, 0, false),
            (1, 0, true),
            (2, 0, false),
            (1, 2, false),
            (2, 3, false),
            (2, 3, false),
        ];
        let (start, readers) = index_readers(&reads, 3);
        assert_eq!(start, vec![0, 0, 2, 4]);
        let got: Vec<(u32, bool)> = readers.iter().map(|r| (r.target, r.bound)).collect();
        assert_eq!(got, vec![(0, true), (2, false), (0, false), (3, false)]);
    }
}
