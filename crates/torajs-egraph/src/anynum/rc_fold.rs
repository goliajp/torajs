//! Refcount no-ops on proven numbers. `rc_inc_any` / `rc_dec_any` of
//! an `any` touch nothing when the value is a number encoding; where
//! every path to the operation has shown the value to be one — the
//! true edge of an `any_is_number` or `any_is_int32` test of it (or
//! of an `and` of such tests), or a def that boxes a number — the
//! operation is deleted. The addend of a `+` is the shape: it is
//! retained when read out of the array, tested as a number, added,
//! and released after the add, and the release ran its tag test
//! again and counted as a call site for register allocation.
//!
//! A forward must-analysis over the blocks: the set of values proven
//! at a block's entry is the intersection over its predecessors of
//! what each proved at its exit plus what its edge proves; a def
//! kills, a box or a copy of a proven value proves. Runs on the typed
//! number nodes, before they expand. A box whose only reader was a
//! deleted release goes with it — left behind, it would still be
//! materialized every iteration.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    ANY_SLOT_TAG_HEAP, BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand,
    TAG_TYPE_NUMBER, Terminator, Type, ValueId,
};

use crate::anynum::expand::is_number_node;
use crate::float_demote::cfg::{successors, term_operand_value};
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RcFoldStats {
    pub rcs_removed: u32,
    pub defs_removed: u32,
}

pub fn fold_number_rcs(module: &mut Module) -> RcFoldStats {
    let mut stats = RcFoldStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let removed = fold_in_function(func);
        stats.rcs_removed += removed;
        if removed > 0 {
            stats.defs_removed += drop_dead_pure(func);
        }
    }
    stats
}

/// The values a true branch on `c` proves to be numbers: the tests
/// `c` is made of must sit in the branching block, each with no def
/// of its operand between it and the branch — a cell stored again
/// after its test holds a value the test never saw. Read in the
/// typed form (`any_is_number` / `any_is_int32`) and in the bit form
/// those expand to (`(bits & TAG) != 0`, `(bits >> 48) == TAG >> 48`,
/// the constant possibly hoisted into a `copy`).
fn proven_by(
    insts: &[Inst],
    end: usize,
    c: ValueId,
    consts: &HashMap<ValueId, i64>,
    out: &mut Vec<ValueId>,
) {
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
        Operand::Value(v) => consts.get(v).copied(),
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

/// What a def proves about its result: a box of a number, or a copy
/// of a proven value.
fn proves(kind: &InstKind, facts: &HashSet<ValueId>) -> bool {
    match kind {
        InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
        InstKind::BoxAny(t, _) => *t != ANY_SLOT_TAG_HEAP,
        InstKind::Copy(_, Operand::Value(s)) => facts.contains(s),
        _ => false,
    }
}

/// The facts at the block's exit from those at its entry.
fn transfer(func: &Function, b: BlockId, mut facts: HashSet<ValueId>) -> HashSet<ValueId> {
    for inst in &func.blocks[b.0 as usize].insts {
        if let Some(r) = inst.result {
            let proven = proves(&inst.kind, &facts);
            facts.remove(&r);
            if proven {
                facts.insert(r);
            }
        }
    }
    facts
}

/// What the number tests of a function prove: the values proven
/// numbers at each block's entry (`None` for a block never reached)
/// and, per block, the values its true edge proves.
pub(super) struct NumberFacts {
    live_in: Vec<Option<HashSet<ValueId>>>,
    edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>>,
}

impl NumberFacts {
    /// The forward must-analysis over `func`.
    pub(super) fn compute(func: &Function) -> Self {
        let n = func.blocks.len();
        let consts = int_consts(func);
        // what the true edge out of a block proves
        let mut edge_proofs: Vec<Option<(BlockId, Vec<ValueId>)>> = vec![None; n];
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
                    edge_proofs[block.id.0 as usize] = Some((*then_blk, proven));
                }
            }
        }
        // None is the top of the lattice: not yet reached
        let mut live_in: Vec<Option<HashSet<ValueId>>> = vec![None; n];
        live_in[0] = Some(HashSet::default());
        let mut changed = true;
        while changed {
            changed = false;
            for bi in 0..n {
                let Some(entry) = live_in[bi].clone() else {
                    continue;
                };
                let out = transfer(func, BlockId(bi as u32), entry);
                for s in successors(&func.blocks[bi].term) {
                    let mut via = out.clone();
                    if let Some((t, proven)) = &edge_proofs[bi]
                        && *t == s
                    {
                        via.extend(proven.iter().copied());
                    }
                    let next = match &live_in[s.0 as usize] {
                        None => via,
                        Some(cur) => cur.intersection(&via).copied().collect(),
                    };
                    if live_in[s.0 as usize].as_ref() != Some(&next) {
                        live_in[s.0 as usize] = Some(next);
                        changed = true;
                    }
                }
            }
        }
        NumberFacts {
            live_in,
            edge_proofs,
        }
    }

    /// Is `v` proven a number on the edge from `b` to `s`: at `b`'s
    /// exit, or by the test `b`'s true edge into `s` passes?
    pub(super) fn proven_on_edge(&self, func: &Function, b: usize, s: BlockId, v: ValueId) -> bool {
        let Some(entry) = &self.live_in[b] else {
            // an unreached block runs nothing
            return true;
        };
        if transfer(func, BlockId(b as u32), entry.clone()).contains(&v) {
            return true;
        }
        matches!(&self.edge_proofs[b], Some((t, proven)) if *t == s && proven.contains(&v))
    }
}

fn fold_in_function(func: &mut Function) -> u32 {
    let has_rc = func.blocks.iter().flat_map(|b| b.insts.iter()).any(|i| {
        matches!(
            i.kind,
            InstKind::RcIncAny(Operand::Value(_)) | InstKind::RcDecAny(Operand::Value(_))
        )
    });
    if !has_rc {
        return 0;
    }
    let n = func.blocks.len();
    let live_in = NumberFacts::compute(func).live_in;
    let mut removed = 0u32;
    for (bi, entry) in live_in.iter().enumerate().take(n) {
        let Some(mut facts) = entry.clone() else {
            continue;
        };
        let insts = std::mem::take(&mut func.blocks[bi].insts);
        let mut kept = Vec::with_capacity(insts.len());
        for inst in insts {
            if let InstKind::RcIncAny(Operand::Value(v)) | InstKind::RcDecAny(Operand::Value(v)) =
                &inst.kind
                && facts.contains(v)
            {
                removed += 1;
                continue;
            }
            if let Some(r) = inst.result {
                let proven = proves(&inst.kind, &facts);
                facts.remove(&r);
                if proven {
                    facts.insert(r);
                }
            }
            kept.push(inst);
        }
        func.blocks[bi].insts = kept;
    }
    removed
}

/// Pure defs nothing reads any more: boxes, casts and copies, to a
/// fixpoint (a cast feeding a dropped box goes next).
fn drop_dead_pure(func: &mut Function) -> u32 {
    let mut dropped = 0u32;
    loop {
        let mut used: HashSet<ValueId> = HashSet::default();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            used.extend(term_operand_value(&block.term));
        }
        let before = dropped;
        for block in &mut func.blocks {
            block.insts.retain(|inst| {
                let dead = match (inst.result, &inst.kind) {
                    (Some(r), kind) if !used.contains(&r) => {
                        is_number_node(kind)
                            || matches!(
                                kind,
                                InstKind::BoxAny(..)
                                    | InstKind::BitCastF64ToI64(_)
                                    | InstKind::BitCastI64ToF64(_)
                                    | InstKind::SiToFp(_)
                                    | InstKind::Copy(_, _)
                            )
                    }
                    _ => false,
                };
                dropped += u32::from(dead);
                !dead
            });
        }
        if dropped == before {
            return dropped;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Inst, Type, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn inst(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }

    /// `%0` an any parameter: tested, then released on both arms; the
    /// true arm's release goes, the false arm's and the join's stay.
    /// A boxed undefined only a release read goes with its release.
    #[test]
    fn a_release_behind_the_number_test_goes() {
        let values = [Type::Any, Type::Bool, Type::Any, Type::Any]
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect();
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![inst(Some(1), InstKind::AnyIsNumber(v(0)))],
                term: Terminator::CondBr {
                    cond: v(1),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(None, InstKind::RcDecAny(v(0))),
                    inst(Some(2), InstKind::Copy(Type::Any, v(0))),
                    inst(None, InstKind::RcIncAny(v(2))),
                    inst(Some(3), InstKind::BoxAny(5, Operand::ConstI64(0))),
                    inst(None, InstKind::RcDecAny(v(3))),
                ],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: vec![inst(None, InstKind::RcDecAny(v(0)))],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(None, InstKind::RcDecAny(v(0)))],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = Module::default();
        m.funcs.push(Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        });
        let st = fold_number_rcs(&mut m);
        assert_eq!(st.rcs_removed, 3);
        // the copy of the parameter and the undefined box
        assert_eq!(st.defs_removed, 2);
        let f = &m.funcs[0];
        assert!(f.blocks[1].insts.is_empty());
        assert_eq!(f.blocks[2].insts.len(), 1);
        assert_eq!(f.blocks[3].insts.len(), 1);
    }
    /// The cell `%1` is tested, then stored again from the opaque
    /// parameter before the branch; the test in a block the branch
    /// does not end proves nothing either. No release goes.
    #[test]
    fn a_test_the_branch_cannot_see_proves_nothing() {
        let values = [Type::Any, Type::Any, Type::Bool, Type::Bool]
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect();
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(Some(1), InstKind::BoxInt32(Operand::ConstI64(1))),
                    inst(Some(2), InstKind::AnyIsNumber(v(1))),
                    inst(Some(1), InstKind::Copy(Type::Any, v(0))),
                    inst(Some(3), InstKind::AnyIsNumber(v(0))),
                ],
                term: Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(1),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(None, InstKind::RcDecAny(v(1)))],
                term: Terminator::Br(BlockId(2)),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(3),
                    else_blk: BlockId(4),
                },
            },
            Block {
                id: BlockId(3),
                insts: vec![inst(None, InstKind::RcDecAny(v(0)))],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let mut m = Module::default();
        m.funcs.push(Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        });
        let st = fold_number_rcs(&mut m);
        assert_eq!(st.rcs_removed, 0);
    }
}
