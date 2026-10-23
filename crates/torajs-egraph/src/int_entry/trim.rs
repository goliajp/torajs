//! The pipeline tail's look back at the loops [`version_loops`] copied.
//!
//! The integer bound the fast copy compares with is only a fact; what
//! the copy is worth is what the passes after the versioning made of
//! it. A loop whose fast copy came out of them no shorter than the
//! slow one gains nothing per iteration (the f64 test it no longer
//! makes is one `icmp` for one `fcmp`), and the second copy is code
//! the rest of the build lays out, allocates and emits for nothing.
//! Such a loop is put back to one copy: the chain's first check
//! branches straight into the slow loop, and the fast copy and the
//! rest of the chain go.
//!
//! What decides is whether the passes treated the two copies alike:
//! walked side by side from their ways in, the fast copy has the same
//! blocks, wired the same way, holding the same instructions one for
//! one — the bound's `fcmp` an `icmp` — as the slow one. Any
//! difference is something a pass made of a fact only the fast copy
//! has (a check folded, an instruction narrowed, the loop unrolled or
//! widened), and the copy stays. Read before the loop rotation, after
//! every pass that uses a fact to rewrite a loop body.
//! `TORAJS_INT_ENTRY_TRIM_OFF=1` keeps every copy.
//!
//! [`version_loops`]: super::version_loops

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Terminator, ValueId};

use super::Versioned;
use crate::branch_fold::sweep_unreachable_blocks;
use crate::float_demote::cfg::term_operand_value;
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TrimStats {
    pub kept: u32,
    pub trimmed: u32,
    /// loops whose chain the later passes reshaped past finding
    pub lost: u32,
}

struct Sides {
    head: BlockId,
    slow: BlockId,
    alike: bool,
}

pub(crate) fn trim_copies(module: &mut Module, loops: &[Versioned]) -> TrimStats {
    let mut stats = TrimStats::default();
    for v in loops {
        let func = &mut module.funcs[v.func];
        let Some(sides) = find_sides(func, &v.checks) else {
            stats.lost += 1;
            continue;
        };
        if !sides.alike {
            stats.kept += 1;
            continue;
        }
        func.blocks[sides.head.0 as usize].term = Terminator::Br(sides.slow);
        sweep_unreachable_blocks(func);
        drop_unused_minted(func, &v.minted);
        stats.trimmed += 1;
    }
    stats
}

/// The chain's first block, the slow loop's way in, and whether the
/// copies are alike — `None` when a check is gone or its sides no
/// longer agree.
fn find_sides(func: &Function, checks: &[(ValueId, bool)]) -> Option<Sides> {
    let dense = func
        .blocks
        .iter()
        .enumerate()
        .all(|(i, b)| b.id.0 as usize == i);
    if !dense || checks.is_empty() {
        return None;
    }
    let mut at: HashMap<ValueId, (BlockId, BlockId, BlockId)> = HashMap::default();
    for block in &func.blocks {
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = block.term
        {
            at.insert(c, (block.id, then_blk, else_blk));
        }
    }
    let mut slow = None;
    let mut pass = None;
    for (cond, slow_then) in checks {
        let (_, then_blk, else_blk) = *at.get(cond)?;
        let (s, p) = if *slow_then {
            (then_blk, else_blk)
        } else {
            (else_blk, then_blk)
        };
        if slow.is_some_and(|x| x != s) {
            return None;
        }
        slow = Some(s);
        pass = Some(p);
    }
    let (slow, pass) = (slow?, pass?);
    let head = at[&checks[0].0].0;
    let dom = DominatorTree::compute(func);
    if dom.dominates(slow, pass) || dom.dominates(pass, slow) || !dom.dominates(head, slow) {
        return None;
    }
    Some(Sides {
        head,
        slow,
        alike: alike(func, &dom, slow, pass),
    })
}

/// The blocks under `slow` and under `fast`, walked in step along
/// their successors, match one for one, and the walk leaves both for
/// the same blocks. Each side's walk starts at its loop: past the
/// blocks in front of it that only fall through — on the fast side
/// the one that seeds the copy's cells and values, which the slow
/// loop has no match for.
fn alike<'a>(func: &'a Function, dom: &DominatorTree, slow: BlockId, fast: BlockId) -> bool {
    let start = |mut b: BlockId| {
        while let Terminator::Br(t) = func.blocks[b.0 as usize].term
            && dom.dominates(b, t)
            && t != b
        {
            b = t;
        }
        b
    };
    let mut work = vec![(start(slow), start(fast))];
    let mut paired: HashMap<BlockId, BlockId> = HashMap::default();
    while let Some((a, b)) = work.pop() {
        let (a_in, b_in) = (dom.dominates(slow, a), dom.dominates(fast, b));
        if a_in != b_in || (!a_in && a != b) {
            return false;
        }
        if !a_in {
            continue;
        }
        if let Some(seen) = paired.insert(a, b) {
            if seen != b {
                return false;
            }
            continue;
        }
        let (x, y) = (&func.blocks[a.0 as usize], &func.blocks[b.0 as usize]);
        let ops = |blk: &'a torajs_core::ssa::Block| {
            blk.insts
                .iter()
                .map(|i| &i.kind)
                .filter(|k| !is_constant(k))
        };
        if ops(x).count() != ops(y).count() || !ops(x).zip(ops(y)).all(|(p, q)| same_op(p, q)) {
            return false;
        }
        match (&x.term, &y.term) {
            (Terminator::Br(s), Terminator::Br(t)) => work.push((*s, *t)),
            (
                Terminator::CondBr {
                    then_blk: s1,
                    else_blk: s2,
                    ..
                },
                Terminator::CondBr {
                    then_blk: t1,
                    else_blk: t2,
                    ..
                },
            ) => work.extend([(*s1, *t1), (*s2, *t2)]),
            (Terminator::Ret(_), Terminator::Ret(_))
            | (Terminator::Unreachable, Terminator::Unreachable) => {}
            _ => return false,
        }
    }
    true
}

/// A constant put in a register, which the constant hoisting places
/// in whichever block it likes.
fn is_constant(k: &InstKind) -> bool {
    matches!(k, InstKind::Copy(_, op) if !matches!(op, Operand::Value(_)))
}

/// One instruction stands for the other: the same operation, the same
/// callee, or the bound's comparison made between integers.
fn same_op(p: &InstKind, q: &InstKind) -> bool {
    match (p, q) {
        (InstKind::FCmp(..), InstKind::ICmp(..)) => true,
        (InstKind::Call(f, _), InstKind::Call(g, _)) => f == g,
        _ => std::mem::discriminant(p) == std::mem::discriminant(q),
    }
}

/// The chain's values nothing reads any more, once its other blocks
/// went: what is left of it sits in the first block.
fn drop_unused_minted(func: &mut Function, minted: &std::ops::Range<u32>) {
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
        let mut dropped = false;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let dead = inst
                    .result
                    .is_some_and(|r| minted.contains(&r.0) && !used.contains(&r));
                dropped |= dead;
                !dead
            });
        }
        if !dropped {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::int_entry::tests::counting_loop;
    use crate::int_entry::version_loops_recorded;
    use torajs_core::ssa::{FPred, IPred};

    fn icmps(f: &Function) -> usize {
        f.blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(i.kind, InstKind::ICmp(..)))
            .count()
    }

    #[test]
    fn a_copy_no_pass_changed_goes_with_its_chain() {
        let mut m = counting_loop(FPred::Olt, vec![]);
        let v = version_loops_recorded(&mut m);
        assert_eq!(v.loops.len(), 1);
        let stats = trim_copies(&mut m, &v.loops);
        assert_eq!((stats.kept, stats.trimmed, stats.lost), (0, 1, 0));
        let f = &m.funcs[1];
        // the loop as it was, entered through what is left of the chain
        let Terminator::Br(head) = f.blocks[0].term else {
            panic!("entry terminator");
        };
        assert_eq!(f.blocks[head.0 as usize].term, Terminator::Br(BlockId(1)));
        // its blocks, the chain's first, and the exit storing the out cell
        assert_eq!(f.blocks.len(), 6);
        assert_eq!(icmps(f), 0, "no check of the chain is left");
        let minted = &v.loops[0].minted;
        let left = f
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .any(|i| i.result.is_some_and(|r| minted.contains(&r.0)));
        assert!(!left, "nothing the chain minted is left");
    }

    #[test]
    fn a_copy_a_pass_made_shorter_stays() {
        let mut m = counting_loop(FPred::Olt, vec![]);
        let v = version_loops_recorded(&mut m);
        // the fast header's `sitofp` fed only the f64 test it no
        // longer makes; a later pass drops it
        let f = &mut m.funcs[1];
        let header = f
            .blocks
            .iter()
            .position(|b| {
                b.insts
                    .iter()
                    .any(|i| matches!(i.kind, InstKind::ICmp(IPred::Slt, ..)))
            })
            .expect("the fast header");
        f.blocks[header]
            .insts
            .retain(|i| !matches!(i.kind, InstKind::SiToFp(_)));
        let blocks = f.blocks.len();
        let stats = trim_copies(&mut m, &v.loops);
        assert_eq!((stats.kept, stats.trimmed, stats.lost), (1, 0, 0));
        assert_eq!(m.funcs[1].blocks.len(), blocks);
    }

    #[test]
    fn a_chain_a_pass_folded_is_left_alone() {
        let mut m = counting_loop(FPred::Olt, vec![]);
        let v = version_loops_recorded(&mut m);
        let f = &mut m.funcs[1];
        let (cond, _) = v.loops[0].checks[0];
        for b in f.blocks.iter_mut() {
            if let Terminator::CondBr {
                cond: Operand::Value(c),
                else_blk,
                ..
            } = b.term
                && c == cond
            {
                b.term = Terminator::Br(else_blk);
            }
        }
        let stats = trim_copies(&mut m, &v.loops);
        assert_eq!((stats.kept, stats.trimmed, stats.lost), (0, 0, 1));
    }
}
