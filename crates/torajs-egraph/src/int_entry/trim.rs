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
//!
//! One difference is the integer test itself: the `sitofp` of the
//! counter that only the f64 test read is dead in the fast copy. A
//! loop whose copy bought nothing else gets that test without a copy
//! (`exit_test`, which `exit_split` then finishes): it goes back to one
//! loop and the integer test goes in front of the f64 one — when the
//! loop sits inside no other, as `exit_test` itself asks.
//! `TORAJS_INT_ENTRY_TRIM_OFF=1` keeps every copy.
//!
//! [`version_loops`]: super::version_loops

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{Block, BlockId, Function, InstKind, Module, Operand, Terminator, ValueId};

use super::Versioned;
use super::exit_test::{self, TestAt};
use super::guard::Round;
use crate::branch_fold::sweep_unreachable_blocks;
use crate::float_demote::cfg::{successors, term_operand_value};
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TrimStats {
    pub kept: u32,
    pub trimmed: u32,
    /// loops whose chain the later passes reshaped past finding
    pub lost: u32,
    /// of the trimmed, loops whose copy bought only its integer exit
    /// tests, which the loop now makes in front of the f64 ones
    pub exit_tests: u32,
}

struct Sides {
    head: BlockId,
    slow: BlockId,
    /// `None` when the copies differ; else the slow loop's tests whose
    /// `sitofp` the fast copy no longer computes
    alike: Option<Vec<ValueId>>,
}

pub(crate) fn trim_copies(module: &mut Module, loops: &[Versioned]) -> TrimStats {
    let mut stats = TrimStats::default();
    for v in loops {
        let func = &mut module.funcs[v.func];
        let Some(sides) = find_sides(func, &v.checks) else {
            stats.lost += 1;
            continue;
        };
        let tests = match &sides.alike {
            None => None,
            Some(dropped) if dropped.is_empty() => Some(Vec::new()),
            Some(dropped) => header_tests(func, &v.tests, dropped),
        };
        let Some(tests) = tests else {
            stats.kept += 1;
            continue;
        };
        func.blocks[sides.head.0 as usize].term = Terminator::Br(sides.slow);
        sweep_unreachable_blocks(func);
        drop_unused_minted(func, &v.minted);
        stats.trimmed += 1;
        if !tests.is_empty() {
            install_exit_tests(func, &tests);
            stats.exit_tests += 1;
        }
    }
    stats
}

/// The tests of a loop whose copy differs from it only in not making
/// them in f64, when each is one the integer exit test serves: they
/// are exactly the loop's tests against the bound, none compares with
/// a value the loop also reads, and each branches out of every loop it
/// sits in. The f64 test then runs once on the way out; a test that
/// only leaves an inner loop would run it once per outer round, where
/// the copy runs it never (see `exit_test`).
fn header_tests(func: &Function, tests: &[TestAt], dropped: &[ValueId]) -> Option<Vec<TestAt>> {
    let all_tests = dropped.iter().all(|d| tests.iter().any(|t| t.test == *d));
    if tests.is_empty()
        || !all_tests
        || tests
            .iter()
            .any(|t| t.round == Round::Exact || !dropped.contains(&t.test))
    {
        return None;
    }
    let mut reads: HashMap<ValueId, u32> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| *reads.entry(v).or_default() += 1);
        }
        if let Some(v) = term_operand_value(&block.term) {
            *reads.entry(v).or_default() += 1;
        }
    }
    let read_once = |v: ValueId| reads.get(&v) == Some(&1);
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let served = tests.iter().all(|t| {
        let Some(b) = branching_on(func, t.test) else {
            return false;
        };
        let images = test_images(&func.blocks[b.0 as usize]);
        leaves_every_loop(func, &loops, b)
            && read_once(t.test)
            && images.len() == 1
            && images.iter().all(|i| read_once(*i))
    });
    served.then(|| tests.to_vec())
}

/// One of the block's successors lies outside every loop holding it.
fn leaves_every_loop(func: &Function, loops: &LoopAnalysis, b: BlockId) -> bool {
    let holding: Vec<_> = loops.loops().iter().filter(|l| l.contains(&b)).collect();
    successors(&func.blocks[b.0 as usize].term).any(|s| holding.iter().all(|l| !l.contains(&s)))
}

/// The block whose branch reads `test`.
fn branching_on(func: &Function, test: ValueId) -> Option<BlockId> {
    func.blocks.iter().find_map(|b| match b.term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } if c == test => Some(b.id),
        _ => None,
    })
}

/// The integer test in front of each f64 one of the loop left, its
/// bounds computed in front of the outermost loop holding the tests.
fn install_exit_tests(func: &mut Function, tests: &[TestAt]) {
    let at: Vec<(BlockId, TestAt)> = tests
        .iter()
        .filter_map(|t| Some((branching_on(func, t.test)?, *t)))
        .collect();
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let Some(l) = loops
        .loops()
        .iter()
        .filter(|l| l.contains(&at[0].0))
        .max_by_key(|l| l.body.len())
    else {
        return;
    };
    let region: HashSet<BlockId> = l.body.iter().copied().collect();
    let plan = exit_test::of_tests(l.header, region, &at);
    exit_test::apply(func, &plan);
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
/// loop has no match for. A slow block may hold one more instruction
/// per f64 test it branches on: the `sitofp` of the counter that only
/// that test read, which the fast copy's integer test left dead. Those
/// tests are the answer; `None` when the copies differ.
fn alike(
    func: &Function,
    dom: &DominatorTree,
    slow: BlockId,
    fast: BlockId,
) -> Option<Vec<ValueId>> {
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
    let mut dropped = Vec::new();
    while let Some((a, b)) = work.pop() {
        let (a_in, b_in) = (dom.dominates(slow, a), dom.dominates(fast, b));
        if a_in != b_in || (!a_in && a != b) {
            return None;
        }
        if !a_in {
            continue;
        }
        if let Some(seen) = paired.insert(a, b) {
            if seen != b {
                return None;
            }
            continue;
        }
        let (x, y) = (&func.blocks[a.0 as usize], &func.blocks[b.0 as usize]);
        if !same_ops(x, y, &HashSet::default()) {
            let images = test_images(x);
            if images.is_empty() || !same_ops(x, y, &images) {
                return None;
            }
            dropped.extend(term_operand_value(&x.term));
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
            _ => return None,
        }
    }
    Some(dropped)
}

/// The two blocks hold the same instructions one for one, constants
/// put in registers and the slow block's `skip` aside.
fn same_ops(x: &Block, y: &Block, skip: &HashSet<ValueId>) -> bool {
    let (p, q) = (ops(x, skip), ops(y, &HashSet::default()));
    p.len() == q.len() && p.iter().zip(&q).all(|(p, q)| same_op(p, q))
}

fn ops<'b>(blk: &'b Block, skip: &HashSet<ValueId>) -> Vec<&'b InstKind> {
    blk.insts
        .iter()
        .filter(|i| !is_constant(&i.kind) && !i.result.is_some_and(|r| skip.contains(&r)))
        .map(|i| &i.kind)
        .collect()
}

/// The counter's image the block's branch compares in f64.
fn test_images(x: &Block) -> HashSet<ValueId> {
    let Some(test) = term_operand_value(&x.term) else {
        return HashSet::default();
    };
    let compared = x.insts.iter().find_map(|i| match i.kind {
        InstKind::FCmp(_, Operand::Value(l), Operand::Value(r)) if i.result == Some(test) => {
            Some([l, r])
        }
        _ => None,
    });
    let Some(compared) = compared else {
        return HashSet::default();
    };
    let mut images: HashSet<ValueId> = HashSet::default();
    for inst in &x.insts {
        if let (InstKind::SiToFp(_), Some(r)) = (&inst.kind, inst.result)
            && compared.contains(&r)
        {
            images.insert(r);
        }
    }
    images
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
    use torajs_core::ssa::{BinOp, FPred, IPred};

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
    fn a_copy_that_only_tests_in_integers_leaves_an_integer_test() {
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
        let stats = trim_copies(&mut m, &v.loops);
        assert_eq!((stats.kept, stats.trimmed, stats.exit_tests), (0, 1, 1));
        let f = &m.funcs[1];
        // one loop, its header testing `i < nb` or the f64 test
        let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } = f.blocks[1].term
        else {
            panic!("header terminator");
        };
        let either = f.blocks[1].insts.iter().find(|i| i.result == Some(c));
        assert!(matches!(
            either.map(|i| &i.kind),
            Some(InstKind::BinOp(BinOp::Or, ..))
        ));
        assert_eq!(icmps(f), 1);
    }

    #[test]
    fn a_copy_a_pass_rewrote_stays() {
        let mut m = counting_loop(FPred::Olt, vec![]);
        let v = version_loops_recorded(&mut m);
        // a pass narrowed something in the fast body
        let f = &mut m.funcs[1];
        let body = (4..f.blocks.len())
            .find(|b| {
                matches!(f.blocks[*b].term, Terminator::Br(_)) && f.blocks[*b].insts.len() == 4
            })
            .expect("the fast body");
        f.blocks[body].insts.remove(0);
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
