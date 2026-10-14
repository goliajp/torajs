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

/// A set of values as a bitmap over the function's value ids: the
/// facts are cloned and intersected once per block visit, and a word
/// per 64 values keeps that to a few stores.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ValueSet(Vec<u64>);

impl ValueSet {
    fn empty(n_values: usize) -> Self {
        ValueSet(vec![0; n_values.div_ceil(64)])
    }

    pub(super) fn contains(&self, v: ValueId) -> bool {
        let i = v.0 as usize;
        i / 64 < self.0.len() && (self.0[i / 64] >> (i % 64)) & 1 == 1
    }

    pub(super) fn insert(&mut self, v: ValueId) {
        let i = v.0 as usize;
        self.0[i / 64] |= 1 << (i % 64);
    }

    pub(super) fn remove(&mut self, v: ValueId) {
        let i = v.0 as usize;
        self.0[i / 64] &= !(1 << (i % 64));
    }

    fn intersect_with(&mut self, other: &ValueSet) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a &= *b;
        }
    }
}

/// What a def proves about its result: a box of a number, or a copy
/// of a proven value.
pub(super) fn proves(kind: &InstKind, facts: &ValueSet) -> bool {
    match kind {
        InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
        InstKind::BoxAny(t, _) => *t != ANY_SLOT_TAG_HEAP,
        InstKind::Copy(_, Operand::Value(s)) => facts.contains(*s),
        _ => false,
    }
}

/// The facts at the block's exit from those at its entry.
fn transfer(func: &Function, b: usize, facts: &mut ValueSet) {
    for inst in &func.blocks[b].insts {
        if let Some(r) = inst.result {
            let proven = proves(&inst.kind, facts);
            facts.remove(r);
            if proven {
                facts.insert(r);
            }
        }
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
/// numbers at each block's entry and exit (`None` for a block never
/// reached) and, per block, the values its true edge proves.
pub(super) struct NumberFacts {
    pub(super) live_in: Vec<Option<ValueSet>>,
    live_out: Vec<Option<ValueSet>>,
    edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>>,
}

impl NumberFacts {
    /// The forward must-analysis over `func`.
    pub(super) fn compute(func: &Function) -> Self {
        Self::from_edge_proofs(func, edge_proofs(func))
    }

    /// The analysis, or `None` when no branch of `func` proves a value
    /// a number: the facts would then hold only boxes, which
    /// `rc_fold` already acts on.
    pub(super) fn compute_if_any_edge_proof(func: &Function) -> Option<Self> {
        let proofs = edge_proofs(func);
        proofs
            .iter()
            .any(Option::is_some)
            .then(|| Self::from_edge_proofs(func, proofs))
    }

    /// The worklist fixpoint: a block is revisited when its entry set
    /// shrinks.
    fn from_edge_proofs(
        func: &Function,
        edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>>,
    ) -> Self {
        let n = func.blocks.len();
        let empty = ValueSet::empty(func.values.len());
        // None is the top of the lattice: not yet reached
        let mut live_in: Vec<Option<ValueSet>> = vec![None; n];
        let mut live_out: Vec<Option<ValueSet>> = vec![None; n];
        live_in[0] = Some(empty.clone());
        // one buffer for every edge; a block's sets are allocated once,
        // when it is first reached
        let mut via = empty.clone();
        // lowest index first: blocks come mostly in program order, so a
        // block tends to be visited after its forward predecessors
        let mut queued = vec![false; n];
        let mut work: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
        work.push(Reverse(0));
        queued[0] = true;
        while let Some(Reverse(bi)) = work.pop() {
            queued[bi] = false;
            let entry = live_in[bi].as_ref().expect("a queued block was reached");
            let out = live_out[bi].get_or_insert_with(|| empty.clone());
            out.0.copy_from_slice(&entry.0);
            transfer(func, bi, out);
            for s in successors(&func.blocks[bi].term) {
                let si = s.0 as usize;
                via.0.copy_from_slice(&out.0);
                if let Some((t, proven)) = &edge_proofs[bi]
                    && *t == s
                {
                    for v in proven {
                        via.insert(*v);
                    }
                }
                let moved = match &mut live_in[si] {
                    Some(cur) => {
                        via.intersect_with(cur);
                        let moved = *cur != via;
                        cur.0.copy_from_slice(&via.0);
                        moved
                    }
                    None => {
                        live_in[si] = Some(via.clone());
                        true
                    }
                };
                if moved && !queued[si] {
                    queued[si] = true;
                    work.push(Reverse(si));
                }
            }
        }
        NumberFacts {
            live_in,
            live_out,
            edge_proofs,
        }
    }

    /// Is `v` proven a number on the edge from `b` to `s`: at `b`'s
    /// exit, or by the test `b`'s true edge into `s` passes?
    pub(super) fn proven_on_edge(&self, b: usize, s: BlockId, v: ValueId) -> bool {
        let Some(out) = &self.live_out[b] else {
            // an unreached block runs nothing
            return true;
        };
        if out.contains(v) {
            return true;
        }
        matches!(&self.edge_proofs[b], Some((t, proven)) if *t == s && proven.contains(&v))
    }
}
