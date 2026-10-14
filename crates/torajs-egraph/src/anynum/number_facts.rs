//! What the number tests of a function prove: the forward
//! must-analysis `rc_fold` deletes refcount no-ops with and
//! `inc_sink` sinks retains by. The set of values proven at a block's
//! entry is the intersection over its predecessors of what each
//! proved at its exit plus what its edge proves; a def kills, a box
//! or a copy of a proven value proves.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    ANY_SLOT_TAG_HEAP, BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, TAG_TYPE_NUMBER,
    Terminator, Type, ValueId,
};

use crate::float_demote::cfg::successors;

/// The values a true branch on `c` proves to be numbers: the tests
/// `c` is made of must sit in the branching block, each with no def
/// of its operand between it and the branch — a cell stored again
/// after its test holds a value the test never saw. Read in the
/// typed form (`any_is_number` / `any_is_int32`) and in the bit form
/// those expand to (`(bits & TAG) != 0`, `(bits >> 48) == TAG >> 48`,
/// the constant possibly hoisted into a `copy`).
fn proven_by(insts: &[Inst], end: usize, c: ValueId, consts: &IntConsts, out: &mut Vec<ValueId>) {
    let Some(at) = insts[..end].iter().rposition(|i| i.result == Some(c)) else {
        return;
    };
    let unchanged =
        |v: &ValueId, from: usize| !insts[from + 1..].iter().any(|i| i.result == Some(*v));
    // the `ptrtoint` the bit tests read, earlier in the block
    let read_of = |x: ValueId, before: usize| -> Option<(ValueId, usize)> {
        let k = insts[..before].iter().rposition(|i| i.result == Some(x))?;
        match &insts[k].kind {
            InstKind::PtrToInt(Operand::Value(v)) => Some((*v, k)),
            _ => None,
        }
    };
    let const_of = |op: &Operand| match op {
        Operand::ConstI64(k) => Some(*k),
        Operand::Value(v) => consts.get(*v),
        _ => None,
    };
    match &insts[at].kind {
        InstKind::AnyIsNumber(Operand::Value(v)) | InstKind::AnyIsInt32(Operand::Value(v))
            if unchanged(v, at) =>
        {
            out.push(*v);
        }
        InstKind::BinOp(BinOp::And, Operand::Value(a), Operand::Value(b)) => {
            proven_by(insts, at, *a, consts, out);
            proven_by(insts, at, *b, consts, out);
        }
        InstKind::ICmp(IPred::Ne, Operand::Value(t), Operand::ConstI64(0)) => {
            let Some(k) = insts[..at].iter().rposition(|i| i.result == Some(*t)) else {
                return;
            };
            if let InstKind::BinOp(BinOp::And, Operand::Value(bits), Operand::ConstI64(tag)) =
                &insts[k].kind
                && *tag == TAG_TYPE_NUMBER as i64
                && let Some((v, r)) = read_of(*bits, k)
                && unchanged(&v, r)
            {
                out.push(v);
            }
        }
        InstKind::ICmp(IPred::Eq, Operand::Value(top), k) => {
            if const_of(k) != Some((TAG_TYPE_NUMBER >> 48) as i64) {
                return;
            }
            let Some(j) = insts[..at].iter().rposition(|i| i.result == Some(*top)) else {
                return;
            };
            if let InstKind::BinOp(BinOp::LShr, Operand::Value(bits), Operand::ConstI64(48)) =
                &insts[j].kind
                && let Some((v, r)) = read_of(*bits, j)
                && unchanged(&v, r)
            {
                out.push(v);
            }
        }
        _ => {}
    }
}

/// [`int_consts`] of one function, built the first time a test asks:
/// most functions hold no bit test that compares against a cell.
struct IntConsts<'f> {
    func: &'f Function,
    map: std::cell::OnceCell<HashMap<ValueId, i64>>,
}

impl IntConsts<'_> {
    fn get(&self, v: ValueId) -> Option<i64> {
        let map = self.map.get_or_init(|| int_consts(self.func));
        map.get(&v).copied()
    }
}

/// Values defined once, as a copy of an integer constant: the hoisted
/// `iconst` cells the bit tests compare against.
fn int_consts(func: &Function) -> HashMap<ValueId, i64> {
    let mut seen: HashMap<ValueId, Option<i64>> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            let Some(r) = inst.result else {
                continue;
            };
            let k = match &inst.kind {
                InstKind::Copy(Type::I64, Operand::ConstI64(k)) => Some(*k),
                _ => None,
            };
            seen.entry(r).and_modify(|e| *e = None).or_insert(k);
        }
    }
    seen.into_iter()
        .filter_map(|(v, k)| k.map(|k| (v, k)))
        .collect()
}

/// The values a set can hold, as bit positions: only the values the
/// caller asks about and those a chain of copies joins them to. A
/// def's fact reads only its own kind and, for a copy, its source's
/// fact, so the facts of these values do not depend on any other's,
/// and the sets stay a word or two wide however many values the
/// function has.
struct Universe {
    /// bit of each value id, `NONE` outside
    bit: Vec<u32>,
    words: usize,
}

const NONE: u32 = u32::MAX;

impl Universe {
    /// `seeds` and every value connected to one through `copy` defs
    /// (either way: a copy's fact reads its source, and a walk
    /// follows a copy onto its result).
    fn of(func: &Function, seeds: &[ValueId]) -> Self {
        let n = func.values.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn root(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                let up = parent[parent[x as usize] as usize];
                parent[x as usize] = up;
                x = up;
            }
            x
        }
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let (Some(r), InstKind::Copy(_, Operand::Value(s))) = (inst.result, &inst.kind) {
                let (a, b) = (root(&mut parent, r.0), root(&mut parent, s.0));
                if a != b {
                    parent[a as usize] = b;
                }
            }
        }
        let mut wanted = vec![false; n];
        for v in seeds {
            let r = root(&mut parent, v.0);
            wanted[r as usize] = true;
        }
        let mut bit = vec![NONE; n];
        let mut next = 0u32;
        for v in 0..n as u32 {
            if wanted[root(&mut parent, v) as usize] {
                bit[v as usize] = next;
                next += 1;
            }
        }
        Universe {
            bit,
            words: (next as usize).div_ceil(64).max(1),
        }
    }

    fn has(&self, set: &[u64], v: ValueId) -> bool {
        match self.bit[v.0 as usize] {
            NONE => false,
            k => (set[k as usize / 64] >> (k % 64)) & 1 == 1,
        }
    }

    fn insert(&self, set: &mut [u64], v: ValueId) {
        let k = self.bit[v.0 as usize];
        if k != NONE {
            set[k as usize / 64] |= 1 << (k % 64);
        }
    }

    /// One def's effect on `set`: a box of a number or a copy of a
    /// proven value proves its result, any other def kills it.
    fn step(&self, inst: &Inst, set: &mut [u64]) {
        let Some(r) = inst.result else {
            return;
        };
        let k = self.bit[r.0 as usize];
        if k == NONE {
            return;
        }
        let proven = match &inst.kind {
            InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
            InstKind::BoxAny(t, _) => *t != ANY_SLOT_TAG_HEAP,
            InstKind::Copy(_, Operand::Value(s)) => self.has(set, *s),
            _ => false,
        };
        let (w, m) = (k as usize / 64, 1u64 << (k % 64));
        if proven {
            set[w] |= m;
        } else {
            set[w] &= !m;
        }
    }
}

/// The facts at one point of a block, moved forward a def at a time.
pub(super) struct Cursor<'a> {
    universe: &'a Universe,
    set: Vec<u64>,
}

impl Cursor<'_> {
    /// Is `v` proven here? Only the values the facts were computed
    /// for, and the values copies join them to, are ever proven.
    pub(super) fn contains(&self, v: ValueId) -> bool {
        self.universe.has(&self.set, v)
    }

    /// Moves past `inst`.
    pub(super) fn step(&mut self, inst: &Inst) {
        self.universe.step(inst, &mut self.set);
    }
}

/// What the true edge out of each block proves.
fn edge_proofs(func: &Function) -> Vec<Option<(BlockId, Vec<ValueId>)>> {
    let consts = IntConsts {
        func,
        map: std::cell::OnceCell::new(),
    };
    let mut out: Vec<Option<(BlockId, Vec<ValueId>)>> = vec![None; func.blocks.len()];
    for block in &func.blocks {
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &block.term
            && then_blk != else_blk
        {
            let mut proven = Vec::new();
            proven_by(&block.insts, block.insts.len(), *c, &consts, &mut proven);
            if !proven.is_empty() {
                out[block.id.0 as usize] = Some((*then_blk, proven));
            }
        }
    }
    out
}

/// What the number tests of a function prove: the values proven
/// numbers at each block's entry and exit (a block never reached
/// has neither) and, per block, the values its true edge proves.
/// The sets cover only the values asked about ([`Universe`]).
pub(super) struct NumberFacts {
    universe: Universe,
    reached: Vec<bool>,
    /// `universe.words` words per block
    live_in: Vec<u64>,
    live_out: Vec<u64>,
    edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>>,
}

impl NumberFacts {
    /// The forward must-analysis over `func`, for `seeds`.
    pub(super) fn compute(func: &Function, seeds: &[ValueId]) -> Self {
        Self::from_edge_proofs(func, seeds, edge_proofs(func))
    }

    /// The analysis, or `None` when no branch of `func` proves a value
    /// a number: the facts would then hold only boxes, which
    /// `rc_fold` already acts on.
    pub(super) fn compute_if_any_edge_proof(func: &Function, seeds: &[ValueId]) -> Option<Self> {
        let proofs = edge_proofs(func);
        proofs
            .iter()
            .any(Option::is_some)
            .then(|| Self::from_edge_proofs(func, seeds, proofs))
    }

    /// The worklist fixpoint: a block is revisited when its entry set
    /// shrinks.
    fn from_edge_proofs(
        func: &Function,
        seeds: &[ValueId],
        edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>>,
    ) -> Self {
        let n = func.blocks.len();
        let universe = Universe::of(func, seeds);
        let w = universe.words;
        // a block not yet reached is the top of the lattice
        let mut reached = vec![false; n];
        let mut live_in = vec![0u64; n * w];
        let mut live_out = vec![0u64; n * w];
        reached[0] = true;
        let mut via = vec![0u64; w];
        // lowest index first: blocks come mostly in program order, so a
        // block tends to be visited after its forward predecessors
        let mut queued = vec![false; n];
        let mut work: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
        work.push(Reverse(0));
        queued[0] = true;
        while let Some(Reverse(bi)) = work.pop() {
            queued[bi] = false;
            let out = &mut live_out[bi * w..(bi + 1) * w];
            out.copy_from_slice(&live_in[bi * w..(bi + 1) * w]);
            for inst in &func.blocks[bi].insts {
                universe.step(inst, out);
            }
            for s in successors(&func.blocks[bi].term) {
                let si = s.0 as usize;
                via.copy_from_slice(out);
                if let Some((t, proven)) = &edge_proofs[bi]
                    && *t == s
                {
                    for v in proven {
                        universe.insert(&mut via, *v);
                    }
                }
                let cur = &mut live_in[si * w..(si + 1) * w];
                let moved = if reached[si] {
                    let mut moved = false;
                    for (c, x) in cur.iter_mut().zip(&via) {
                        let m = *c & *x;
                        moved |= m != *c;
                        *c = m;
                    }
                    moved
                } else {
                    reached[si] = true;
                    cur.copy_from_slice(&via);
                    true
                };
                if moved && !queued[si] {
                    queued[si] = true;
                    work.push(Reverse(si));
                }
            }
        }
        NumberFacts {
            universe,
            reached,
            live_in,
            live_out,
            edge_proofs,
        }
    }

    /// The facts at the entry of block `b`, or `None` when nothing
    /// reaches it.
    pub(super) fn at_entry(&self, b: usize) -> Option<Cursor<'_>> {
        let w = self.universe.words;
        self.reached[b].then(|| Cursor {
            universe: &self.universe,
            set: self.live_in[b * w..(b + 1) * w].to_vec(),
        })
    }

    /// Is `v` proven a number on the edge from `b` to `s`: at `b`'s
    /// exit, or by the test `b`'s true edge into `s` passes? `v` is
    /// one of the values the facts were computed for, or a value
    /// copies join to one.
    pub(super) fn proven_on_edge(&self, b: usize, s: BlockId, v: ValueId) -> bool {
        if !self.reached[b] {
            // an unreached block runs nothing
            return true;
        }
        let w = self.universe.words;
        if self.universe.has(&self.live_out[b * w..(b + 1) * w], v) {
            return true;
        }
        matches!(&self.edge_proofs[b], Some((t, proven)) if *t == s && proven.contains(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn copy(of: u32) -> InstKind {
        InstKind::Copy(Type::Any, Operand::Value(ValueId(of)))
    }

    /// Only `%4` is asked about. `%4` copies `%3`, which holds a box
    /// copied in `bb0` and a copy of the parameter on `bb1`: the facts
    /// of `%3` are kept although no seed names it, so the arm that kept
    /// the box proves it and the join, which `bb1` also reaches, does
    /// not.
    #[test]
    fn a_seed_reads_the_facts_of_the_copies_it_is_made_of() {
        let values = [Type::Any, Type::Bool, Type::Any, Type::Any, Type::Any]
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect();
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let blocks = vec![
            block(
                0,
                vec![
                    inst(2, InstKind::BoxInt32(Operand::ConstI64(7))),
                    inst(3, copy(2)),
                ],
                Terminator::CondBr {
                    cond: Operand::Value(ValueId(1)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, vec![inst(3, copy(0))], Terminator::Br(BlockId(3))),
            block(2, vec![], Terminator::Br(BlockId(3))),
            block(3, vec![inst(4, copy(3))], Terminator::Ret(None)),
        ];
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        };
        let facts = NumberFacts::compute(&func, &[ValueId(4)]);
        assert!(facts.proven_on_edge(2, BlockId(3), ValueId(3)));
        assert!(!facts.proven_on_edge(1, BlockId(3), ValueId(3)));
        let mut at = facts.at_entry(2).expect("bb2 is reached");
        assert!(at.contains(ValueId(3)));
        at.step(&func.blocks[3].insts[0]);
        assert!(at.contains(ValueId(4)));
        let join = facts.at_entry(3).expect("bb3 is reached");
        assert!(!join.contains(ValueId(3)));
    }
}
