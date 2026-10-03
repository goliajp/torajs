//! A conditional whose answer a branch above it already gave.
//!
//! When block B's only predecessor P ends in `cond_br c, T, E` with
//! T ≠ E, everything B dominates is reached through that one edge, and
//! a single-def `c` has not been computed again since the edge was
//! taken: its definition sits above P, so running it again means
//! leaving B's subtree and coming back in through the same edge. In
//! the subtree `c` is the constant the edge stands for, and a
//! `cond_br c` there goes one way.
//!
//! The dedup is what makes a second test name the first one's value:
//! `a[i].k === X || a[i].k === Y` reads `a`, `i` and the element twice
//! with a nullish guard and a bounds check in front of each read, and
//! once the second set of reads is the first set, its guards branch on
//! the compares the first set already branched on.
//!
//! `TORAJS_KNOWN_BRANCH_OFF=1` skips, `TORAJS_KNOWN_BRANCH_STATS=1`
//! counts.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{
    BlockId, Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::branch_fold::sweep_unreachable_blocks;
use crate::float_demote::cfg::multi_def_values;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KnownBranchStats {
    /// Conditionals turned into a plain branch.
    pub folded: u32,
    /// Blocks only the folded-away arms reached.
    pub blocks_removed: u32,
    /// Address constants nothing reads any more.
    pub refs_removed: u32,
}

pub(crate) fn fold_known_branches(module: &mut Module) -> KnownBranchStats {
    let mut stats = KnownBranchStats::default();
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            fold_fn(func, &mut stats);
        }
    }
    stats
}

/// The fact the edge into `b` carries, when `b` has one way in.
fn edge_fact(
    func: &Function,
    preds: &[Vec<BlockId>],
    multi: &HashSet<ValueId>,
    b: BlockId,
) -> Option<(ValueId, bool)> {
    let [p] = preds[b.0 as usize].as_slice() else {
        return None;
    };
    match &func.blocks[p.0 as usize].term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } if then_blk != else_blk && !multi.contains(c) => Some((*c, *then_blk == b)),
        _ => None,
    }
}

fn fold_fn(func: &mut Function, stats: &mut KnownBranchStats) {
    let dom = DominatorTree::compute(func);
    let n = func.blocks.len();
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for block in &func.blocks {
        let (a, b) = match &block.term {
            Terminator::Br(t) => (Some(*t), None),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => (Some(*then_blk), Some(*else_blk)),
            Terminator::Ret(_) | Terminator::Unreachable => (None, None),
        };
        for s in a.into_iter().chain(b) {
            if (s.0 as usize) < n && !preds[s.0 as usize].contains(&block.id) {
                preds[s.0 as usize].push(block.id);
            }
        }
    }
    let multi = multi_def_values(func);

    enum Visit {
        Enter(BlockId),
        Exit(ValueId),
    }
    let mut known: HashMap<ValueId, bool> = HashMap::new();
    let mut folds: Vec<(usize, BlockId)> = Vec::new();
    let mut stack = vec![Visit::Enter(BlockId(0))];
    while let Some(visit) = stack.pop() {
        let b = match visit {
            Visit::Exit(c) => {
                known.remove(&c);
                continue;
            }
            Visit::Enter(b) => b,
        };
        if let Some((c, taken)) = edge_fact(func, &preds, &multi, b)
            && !known.contains_key(&c)
        {
            known.insert(c, taken);
            stack.push(Visit::Exit(c));
        }
        for &child in dom.children(b) {
            stack.push(Visit::Enter(child));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &func.blocks[b.0 as usize].term
            && let Some(&taken) = known.get(c)
        {
            folds.push((b.0 as usize, if taken { *then_blk } else { *else_blk }));
        }
    }
    if !folds.is_empty() {
        for (b, target) in &folds {
            func.blocks[*b].term = Terminator::Br(*target);
        }
        stats.folded += folds.len() as u32;
        stats.blocks_removed += sweep_unreachable_blocks(func);
    }
    stats.refs_removed += drop_unread_refs(func);
}

/// The dedup leaves a `global_ref` behind when the compare reading it
/// became an earlier compare; one nothing reads is two dead
/// instructions on the path.
fn drop_unread_refs(func: &mut Function) -> u32 {
    let mut read: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                read.insert(v);
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &block.term
        {
            read.insert(*v);
        }
    }
    let mut removed = 0;
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| {
            let dead = matches!(inst.kind, InstKind::GlobalRef(_))
                && inst.result.is_some_and(|r| !read.contains(&r));
            removed += u32::from(dead);
            !dead
        });
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, IPred, Inst, Type, ValueInfo};

    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }
    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }
    fn cond_br(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }
    fn ret(k: i64) -> Terminator {
        Terminator::Ret(Some(Operand::ConstI64(k)))
    }
    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                8
            ],
            blocks,
            current_origin: None,
        }
    }
    fn run(f: &mut Function) -> KnownBranchStats {
        let mut stats = KnownBranchStats::default();
        fold_fn(f, &mut stats);
        stats
    }
    fn test_of_v0() -> Vec<Inst> {
        vec![mk(1, InstKind::ICmp(IPred::Eq, v(0), Operand::ConstI64(0)))]
    }

    /// b0: c = v0 == 0; cond_br c, b1, b2 · b1: ret 1
    /// b2: cond_br c, b3, b4 · b3: ret 3 · b4: ret 4
    #[test]
    fn a_test_repeated_on_the_false_edge_takes_its_false_arm() {
        let mut f = func(vec![
            blk(0, test_of_v0(), cond_br(1, 1, 2)),
            blk(1, vec![], ret(1)),
            blk(2, vec![], cond_br(1, 3, 4)),
            blk(3, vec![], ret(3)),
            blk(4, vec![], ret(4)),
        ]);
        let stats = run(&mut f);
        assert_eq!((stats.folded, stats.blocks_removed), (1, 1));
        assert!(matches!(f.blocks[2].term, Terminator::Br(BlockId(3))));
        assert!(matches!(
            f.blocks[3].term,
            Terminator::Ret(Some(Operand::ConstI64(4)))
        ));
    }

    /// b0: cond_br c, b1, b2 · b1: br b2 · b2: cond_br c, b3, b4 — b2 is
    /// reached on both answers.
    #[test]
    fn a_test_after_a_join_stays() {
        let mut f = func(vec![
            blk(0, test_of_v0(), cond_br(1, 1, 2)),
            blk(1, vec![], Terminator::Br(BlockId(2))),
            blk(2, vec![], cond_br(1, 3, 4)),
            blk(3, vec![], ret(3)),
            blk(4, vec![], ret(4)),
        ]);
        assert_eq!(run(&mut f).folded, 0);
    }

    /// The fact holds below the edge's target, past a join inside its
    /// subtree: b0: cond_br c, b1, b5 · b1: cond_br v2, b2, b3 ·
    /// b2, b3: br b4 · b4: cond_br c, b5, b6.
    #[test]
    fn a_test_dominated_by_the_edge_target_folds_past_a_join() {
        let mut f = func(vec![
            blk(0, test_of_v0(), cond_br(1, 1, 5)),
            blk(1, vec![], cond_br(2, 2, 3)),
            blk(2, vec![], Terminator::Br(BlockId(4))),
            blk(3, vec![], Terminator::Br(BlockId(4))),
            blk(4, vec![], cond_br(1, 5, 6)),
            blk(5, vec![], ret(5)),
            blk(6, vec![], ret(6)),
        ]);
        let stats = run(&mut f);
        assert_eq!(stats.folded, 1);
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(5))));
    }

    /// A loop cell is computed again on the way round: b1 defines %1 a
    /// second time, so the test in b2 is not the one b0 branched on.
    #[test]
    fn a_test_of_a_value_defined_twice_stays() {
        let mut f = func(vec![
            blk(0, test_of_v0(), cond_br(1, 1, 3)),
            blk(
                1,
                vec![mk(1, InstKind::Copy(Type::Bool, Operand::ConstBool(false)))],
                Terminator::Br(BlockId(2)),
            ),
            blk(2, vec![], cond_br(1, 3, 4)),
            blk(3, vec![], ret(3)),
            blk(4, vec![], ret(4)),
        ]);
        assert_eq!(run(&mut f).folded, 0);
    }

    #[test]
    fn an_address_constant_nothing_reads_goes() {
        let mut f = func(vec![blk(
            0,
            vec![
                mk(2, InstKind::GlobalRef("a".into())),
                mk(3, InstKind::GlobalRef("b".into())),
                mk(4, InstKind::Load(Type::I64, v(3), 0)),
            ],
            ret(0),
        )]);
        assert_eq!(run(&mut f).refs_removed, 1);
        assert_eq!(f.blocks[0].insts.len(), 2);
    }
}
