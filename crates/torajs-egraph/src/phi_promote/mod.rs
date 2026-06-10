//! Full mem2reg — φ-placement promotion of loop-carried / join-slot
//! stack cells, the general case `slot_forward` (block-local) and
//! `mem2reg` (single-def-block) leave behind. Textbook pipeline
//! (LLVM `PromoteMemToReg` / Cytron et al.):
//!
//! 1. **φ placement** — iterated dominance frontier of the cell's
//!    store blocks (Cooper-Harvey-Kennedy DF off the existing
//!    domtree).
//! 2. **Rename** — domtree preorder walk threading the reaching def;
//!    loads forward to it, stores update it, successors' φs receive
//!    block-exit incomings. A load (or φ incoming) reached by NO def
//!    aborts the cell — torajs has no `undef` operand and a read of
//!    uninitialized memory must keep its slot.
//! 3. **Trivial-φ elimination** — Braun et al.'s `tryRemoveTrivialPhi`
//!    (φ whose incomings are all the same value or itself), iterated
//!    to a fixpoint, then a mark-sweep so unreferenced φs emit
//!    nothing.
//! 4. **φ destruction** — φs never reach the IR. Each live φ becomes
//!    `InstKind::Copy(ty, incoming)` defining the φ's ValueId at the
//!    end of every predecessor (LLVM PHIElimination's mov shape; the
//!    multi-def Copy contract is documented on the variant). A
//!    predecessor with several successors gets its edge split with a
//!    fresh forwarding block (single-pred φ blocks cannot survive
//!    trivial-φ elim, so split-on-multi-succ is the only case).
//!    Same-edge copies are parallel-copy semantics: sequentialized
//!    ready-first, cycles broken with a fresh temp (the classic
//!    swap-problem fix).
//!
//! Runs AFTER the egraph pass and before rc_peephole: GVN / elaborate
//! never see Copy (their arms are unreachable!), and rc_peephole
//! treats Copy as rc-transparent. Soundness of the slot model itself
//! (no aliasing) comes from `collect_unescaped_slots`, same as
//! slot_forward / mem2reg.

mod destruct;
mod plan;

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::dominator::DominatorTree;
use crate::rc_peephole::collect_unescaped_slots;

use destruct::apply_plans;
use plan::{mark_live_phis, plan_cell, trivial_phi_elim};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PhiPromoteStats {
    pub cells_promoted: u32,
    pub cells_aborted: u32,
    pub phis_placed: u32,
    pub trivial_phis_removed: u32,
    pub loads_forwarded: u32,
    pub copies_inserted: u32,
    pub edges_split: u32,
    pub stores_removed: u32,
    pub slots_removed: u32,
}

/// One φ node during planning. Never becomes an IR inst.
#[derive(Debug)]
struct PhiNode {
    vid: ValueId,
    block: BlockId,
    /// (pred block, incoming operand) — filled during rename.
    incoming: Vec<(BlockId, Operand)>,
    live: bool,
}

/// A fully-planned cell promotion, applied only if rename succeeded.
struct CellPlan {
    slot: ValueId,
    off: u64,
    ty: Type,
    phis: Vec<PhiNode>,
    /// load result → reaching operand (may point at a φ vid).
    replace: HashMap<ValueId, Operand>,
}

pub fn promote_phi_slots(module: &mut Module) -> PhiPromoteStats {
    let mut stats = PhiPromoteStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        promote_in_function(func, &mut stats);
    }
    stats
}

fn promote_in_function(func: &mut Function, stats: &mut PhiPromoteStats) {
    let slots = collect_unescaped_slots(func);
    if slots.is_empty() {
        return;
    }
    let domtree = DominatorTree::compute(func);
    let preds = cfg_preds(func);
    let df = dominance_frontier(func, &domtree, &preds);

    // gather cells with at least one load (others are DSE food only).
    let mut cells: HashMap<(ValueId, u64), (HashSet<BlockId>, Type)> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Store(_, Operand::Value(s), off) if slots.contains(s) => {
                    cells
                        .entry((*s, *off))
                        .or_insert_with(|| (HashSet::new(), Type::I64))
                        .0
                        .insert(block.id);
                }
                InstKind::Load(ty, Operand::Value(s), off) if slots.contains(s) => {
                    cells
                        .entry((*s, *off))
                        .or_insert_with(|| (HashSet::new(), Type::I64))
                        .1 = *ty;
                }
                _ => {}
            }
        }
    }
    // deterministic cell order (HashMap iteration order is not).
    let mut cell_keys: Vec<(ValueId, u64)> = cells.keys().copied().collect();
    cell_keys.sort_by_key(|(v, o)| (v.0, *o));

    let mut plans: Vec<CellPlan> = Vec::new();
    for key in cell_keys {
        let (store_blocks, ty) = &cells[&key];
        if store_blocks.is_empty() || !cell_has_load(func, key) {
            continue;
        }
        match plan_cell(func, &domtree, &preds, &df, key, *ty, store_blocks) {
            Some(plan) => plans.push(plan),
            None => stats.cells_aborted += 1,
        }
    }
    if plans.is_empty() {
        return;
    }
    for plan in &mut plans {
        stats.phis_placed += plan.phis.len() as u32;
        stats.trivial_phis_removed += trivial_phi_elim(plan);
        mark_live_phis(plan);
    }
    apply_plans(func, plans, &slots, stats);
}

/// CFG predecessor table (terminator successors reversed).
fn cfg_preds(func: &Function) -> Vec<Vec<BlockId>> {
    let n = func.blocks.len();
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for block in &func.blocks {
        for succ in term_succs(&block.term) {
            preds[succ.0 as usize].push(block.id);
        }
    }
    preds
}

fn term_succs(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(b) => vec![*b],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

/// Cooper-Harvey-Kennedy dominance frontier: for each join block J
/// (≥2 preds), walk each pred's idom chain up to (excluding)
/// idom(J), adding J to every walked block's frontier.
fn dominance_frontier(
    func: &Function,
    domtree: &DominatorTree,
    preds: &[Vec<BlockId>],
) -> Vec<HashSet<BlockId>> {
    let n = func.blocks.len();
    let mut df: Vec<HashSet<BlockId>> = vec![HashSet::new(); n];
    for join in func.blocks.iter().map(|b| b.id) {
        let jp = &preds[join.0 as usize];
        if jp.len() < 2 {
            continue;
        }
        let stop = domtree.immediate_dominator(join);
        for &p in jp {
            if !domtree.is_reachable(p) {
                continue;
            }
            let mut runner = p;
            loop {
                if Some(runner) == stop {
                    break;
                }
                df[runner.0 as usize].insert(join);
                match domtree.immediate_dominator(runner) {
                    Some(d) => runner = d,
                    None => break, // reached entry
                }
            }
        }
    }
    df
}

fn cell_has_load(func: &Function, (slot, off): (ValueId, u64)) -> bool {
    func.blocks.iter().any(|b| {
        b.insts.iter().any(|i| {
            matches!(&i.kind, InstKind::Load(_, Operand::Value(s), o)
                if *s == slot && *o == off)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::FuncId;
    use torajs_core::ssa::{Block, Inst, ValueInfo};

    fn val(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn store(v: Operand, slot: u32) -> Inst {
        void(InstKind::Store(v, Operand::Value(ValueId(slot)), 0))
    }

    fn load(result: u32, slot: u32) -> Inst {
        val(
            result,
            InstKind::Load(Type::I64, Operand::Value(ValueId(slot)), 0),
        )
    }

    fn func_of(n_values: usize, blocks: Vec<Block>) -> Module {
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::Void,
                blocks,
                values: vec![
                    ValueInfo {
                        ty: Type::I64,
                        name: None
                    };
                    n_values
                ],
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond_br(cond: Operand, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond,
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    fn count_kind(m: &Module, f: impl Fn(&InstKind) -> bool) -> usize {
        m.funcs[0]
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| f(&i.kind))
            .count()
    }

    /// gcd's loop-carried shape — init store in bb0, header load in
    /// bb1, body re-store in bb2 (back-edge), exit load in bb3. One φ
    /// at the header; loads forward to it; Copys land at bb0 and bb2
    /// ends; slot dissolves.
    #[test]
    fn loop_carried_slot_promotes() {
        let mut m = func_of(
            4,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(9), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    cond_br(Operand::Value(ValueId(1)), 2, 3),
                ),
                block(
                    2,
                    vec![
                        val(
                            2,
                            InstKind::BinOp(
                                torajs_core::ssa::BinOp::Add,
                                Operand::Value(ValueId(1)),
                                Operand::ConstI64(1),
                            ),
                        ),
                        store(Operand::Value(ValueId(2)), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    3,
                    vec![load(3, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                ),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 1);
        assert_eq!(stats.phis_placed, 1);
        assert_eq!(stats.loads_forwarded, 2);
        assert_eq!(stats.slots_removed, 1);
        assert_eq!(stats.edges_split, 0);
        // both stores gone, both loads gone, 2 Copys (entry + back-edge).
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Store(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Load(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Alloca(_))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Copy(..))), 2);
        // bb0 and bb2 each end with a Copy defining the SAME vid.
        let f = &m.funcs[0];
        let copy_dst = |b: usize| match f.blocks[b].insts.last() {
            Some(Inst {
                result: Some(r),
                kind: InstKind::Copy(..),
                ..
            }) => *r,
            other => panic!("expected trailing Copy in bb{b}, got {other:?}"),
        };
        assert_eq!(copy_dst(0), copy_dst(2), "one φ home across both defs");
        // the add in bb2 now reads the φ vid, not a load.
        let InstKind::BinOp(_, a, _) = &f.blocks[2].insts[0].kind else {
            panic!("expected binop");
        };
        assert_eq!(*a, Operand::Value(copy_dst(0)));
    }

    /// fib40-main's multi-ret join shape — diamond stores in bb1/bb2,
    /// join load in bb3. φ at the join, Copys at both arm ends.
    #[test]
    fn join_slot_promotes() {
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![val(0, InstKind::Alloca(Type::I64))],
                    cond_br(Operand::ConstBool(true), 1, 2),
                ),
                block(
                    1,
                    vec![store(Operand::ConstI64(7), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    2,
                    vec![store(Operand::ConstI64(8), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 1);
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.edges_split, 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Copy(..))), 2);
        let f = &m.funcs[0];
        assert!(matches!(
            f.blocks[1].insts.last().unwrap().kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(7))
        ));
        assert!(matches!(
            f.blocks[2].insts.last().unwrap().kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(8))
        ));
        // ret reads the φ vid.
        let Terminator::Ret(Some(Operand::Value(r))) = f.blocks[3].term else {
            panic!("expected value ret");
        };
        assert_eq!(Some(r), f.blocks[1].insts.last().unwrap().result);
    }

    /// One diamond arm stores, the other doesn't — the join φ would
    /// need an undef incoming. Cell must abort untouched.
    #[test]
    fn undef_path_aborts() {
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![val(0, InstKind::Alloca(Type::I64))],
                    cond_br(Operand::ConstBool(true), 1, 2),
                ),
                block(
                    1,
                    vec![store(Operand::ConstI64(5), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                block(2, vec![], Terminator::Br(BlockId(3))),
                block(
                    3,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 0);
        assert_eq!(stats.cells_aborted, 1);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Copy(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Load(..))), 1);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Store(..))), 1);
    }

    /// Both loop paths carry the same value — the header φ is trivial
    /// and collapses; loads forward straight to the init value with
    /// zero Copys.
    #[test]
    fn trivial_phi_collapses() {
        let mut m = func_of(
            3,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(3), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    cond_br(Operand::Value(ValueId(1)), 2, 3),
                ),
                block(
                    2,
                    // re-store the loaded value — same e-value flows
                    // around the back edge.
                    vec![store(Operand::Value(ValueId(1)), 0)],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(None)),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 1);
        assert_eq!(stats.trivial_phis_removed, 1);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Copy(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Load(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Store(..))), 0);
        // the CondBr now reads the original constant through the chain.
        assert!(matches!(
            m.funcs[0].blocks[1].term,
            Terminator::CondBr {
                cond: Operand::ConstI64(3),
                ..
            }
        ));
    }

    /// A CondBr predecessor feeding a φ block — its edge must split
    /// so the Copy doesn't execute on the other arm.
    #[test]
    fn critical_edge_splits() {
        // bb0: store 1; cond_br bb1, bb2   (bb2 is the φ join)
        // bb1: store 2; br bb2
        // bb2: load
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(1), 0),
                    ],
                    cond_br(Operand::ConstBool(true), 1, 2),
                ),
                block(
                    1,
                    vec![store(Operand::ConstI64(2), 0)],
                    Terminator::Br(BlockId(2)),
                ),
                block(
                    2,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 1);
        assert_eq!(stats.edges_split, 1);
        let f = &m.funcs[0];
        assert_eq!(f.blocks.len(), 4, "split block appended");
        // bb0's else-arm (was bb2) now routes through the new block.
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = f.blocks[0].term
        else {
            panic!("expected cond_br");
        };
        assert_eq!(then_blk, BlockId(1), "non-φ arm untouched");
        assert_eq!(else_blk, BlockId(3), "φ arm redirected to split block");
        // the split block holds bb0's incoming Copy and jumps to bb2.
        assert!(matches!(
            f.blocks[3].insts[..],
            [Inst {
                kind: InstKind::Copy(Type::I64, Operand::ConstI64(1)),
                ..
            }]
        ));
        assert!(matches!(f.blocks[3].term, Terminator::Br(BlockId(2))));
        // bb1 (single-succ pred) carries its Copy inline.
        assert!(matches!(
            f.blocks[1].insts.last().unwrap().kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(2))
        ));
    }

    /// Two cells swapped around a loop back-edge (a, b = b, a) — the
    /// same-edge parallel copies form a cycle that needs a temp.
    #[test]
    fn swap_cycle_uses_temp() {
        // bb0: store 1→A; store 2→B; br bb1
        // bb1: %a=load A; %b=load B; store %b→A; store %a→B; cond_br bb1, bb2
        // bb2: ret %a
        let mut m = func_of(
            4,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        val(1, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(1), 0),
                        store(Operand::ConstI64(2), 1),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        load(2, 0),
                        load(3, 1),
                        store(Operand::Value(ValueId(3)), 0),
                        store(Operand::Value(ValueId(2)), 1),
                    ],
                    cond_br(Operand::Value(ValueId(2)), 1, 2),
                ),
                block(2, vec![], Terminator::Ret(Some(Operand::Value(ValueId(2))))),
            ],
        );
        let n_values_before = m.funcs[0].values.len();
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_promoted, 2);
        assert_eq!(stats.phis_placed, 2);
        // back-edge carries the swap cycle: 2 φ copies + 1 temp-saving
        // copy; the entry edge carries 2 plain copies.
        assert_eq!(stats.copies_inserted, 5);
        // a temp vid was allocated beyond the 2 φ vids.
        assert!(m.funcs[0].values.len() >= n_values_before + 3);
        // semantic spot-check: no Copy chain writes a dst that a later
        // same-edge Copy still reads (sequentialization correctness).
        let f = &m.funcs[0];
        let bb1 = &f.blocks[1];
        let copies: Vec<(ValueId, Operand)> = bb1
            .insts
            .iter()
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Copy(_, src), Some(dst)) => Some((dst, *src)),
                _ => None,
            })
            .collect();
        for (i, (dst, _)) in copies.iter().enumerate() {
            for (_, later_src) in &copies[i + 1..] {
                assert_ne!(
                    *later_src,
                    Operand::Value(*dst),
                    "copy clobbers a value a later copy reads"
                );
            }
        }
    }

    /// Escaped slot (pointer passed to a call) must stay untouched.
    #[test]
    fn escaped_slot_untouched() {
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(1), 0),
                        void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(0))])),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0), store(Operand::Value(ValueId(1)), 0)],
                    cond_br(Operand::Value(ValueId(1)), 1, 2),
                ),
                block(2, vec![], Terminator::Ret(None)),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats, PhiPromoteStats::default());
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Load(..))), 1);
    }

    /// The inlined-gcd shape that motivated pruning: an inner loop's
    /// slot (init in bb2, back-edge store in bb4, loads only in bb3)
    /// nested inside an outer loop whose header bb1 is in the slot's
    /// iterated dominance frontier. Unpruned placement would put a φ
    /// at bb1 where the slot is dead-and-uninitialized (bb0's incoming
    /// has no def) and abort the whole cell; live-in pruning skips bb1
    /// and the cell promotes.
    #[test]
    fn dead_outer_join_does_not_abort() {
        let mut m = func_of(
            3,
            vec![
                block(
                    0,
                    vec![val(0, InstKind::Alloca(Type::I64))],
                    Terminator::Br(BlockId(1)),
                ),
                // outer loop header — join of bb0 and bb5, slot dead here.
                block(1, vec![], Terminator::Br(BlockId(2))),
                block(
                    2,
                    vec![store(Operand::ConstI64(1), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                // inner loop header — the only live-in join.
                block(
                    3,
                    vec![load(1, 0)],
                    cond_br(Operand::Value(ValueId(1)), 4, 5),
                ),
                block(
                    4,
                    vec![store(Operand::ConstI64(2), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                // outer back-edge / exit split.
                block(5, vec![], cond_br(Operand::ConstBool(true), 1, 6)),
                block(6, vec![], Terminator::Ret(None)),
            ],
        );
        let stats = promote_phi_slots(&mut m);
        assert_eq!(stats.cells_aborted, 0, "pruning must avoid the dead join");
        assert_eq!(stats.cells_promoted, 1);
        assert_eq!(stats.phis_placed, 1, "only the inner header gets a φ");
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Load(..))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Alloca(_))), 0);
        assert_eq!(count_kind(&m, |k| matches!(k, InstKind::Copy(..))), 2);
    }
}
