//! φ-constant jump threading — the edge redirection half of LLVM
//! JumpThreading, without block duplication.
//!
//! A generator machine's `continue __sm` goto ends its block with
//! `state = copy K; br head`, and the head re-tests `state` against
//! each label in a chain of one-compare blocks. Along that edge every
//! compare is already decided by K, so the edge can go straight to the
//! arm the chain would pick.
//!
//! For a block P ending in `c = copy CONST; br H`, walk from H through
//! blocks that are either empty with a `br`, or hold exactly one
//! `r = icmp pred c, CONST'` (either operand order) whose only use is
//! their own `cond_br r`; each of those is decided, so follow the
//! chosen successor. Where the walk stops, P jumps instead. No
//! definition moves or disappears: the skipped blocks define only the
//! compare results they consume themselves, so every value reaches the
//! new target with the definition it had on the old path. Blocks left
//! without predecessors are swept.
//!
//! An edge from outside a loop never lands inside it past its header:
//! `i = 0; br head; head: i < n` would otherwise jump straight into the
//! body, peeling the first test off every counted loop and handing the
//! loop-shape passes after this one (ctpop, float_demote, rotation) a
//! loop they no longer recognize. The machine's gotos come from inside
//! the dispatch loop, so they are not affected.
//!
//! `TORAJS_PHI_THREAD_OFF=1` skips, `TORAJS_PHI_THREAD_STATS=1` counts.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::branch_fold::sweep_unreachable_blocks;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

#[derive(Debug, Default)]
pub struct PhiThreadStats {
    /// Edges redirected past at least one decided block.
    pub edges: u32,
    /// Blocks swept once no edge reached them.
    pub blocks_swept: u32,
}

/// Bound on decided blocks walked from one edge; a chain longer than
/// this keeps its tail.
const MAX_STEPS: usize = 32;

pub fn thread_phi_constants(module: &mut Module) -> PhiThreadStats {
    let mut stats = PhiThreadStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let edges = thread_fn(func);
        if edges > 0 {
            stats.edges += edges;
            stats.blocks_swept += sweep_unreachable_blocks(func);
        }
    }
    stats
}

fn thread_fn(func: &mut Function) -> u32 {
    let uses = use_counts(func);
    let dom = DominatorTree::compute(func);
    let loops: Vec<(BlockId, HashSet<BlockId>)> = LoopAnalysis::compute(func, &dom)
        .loops()
        .iter()
        .map(|l| (l.header, l.body.iter().copied().collect()))
        .collect();
    let enters_loop_body = |p: BlockId, t: BlockId| {
        loops
            .iter()
            .any(|(h, body)| t != *h && body.contains(&t) && !body.contains(&p))
    };
    let mut edges = 0;
    for p in 0..func.blocks.len() {
        let Terminator::Br(head) = func.blocks[p].term else {
            continue;
        };
        let Some((cell, k)) = func.blocks[p]
            .insts
            .last()
            .and_then(|i| match (&i.kind, i.result) {
                (InstKind::Copy(_, Operand::ConstI64(k)), Some(c)) => Some((c, *k)),
                _ => None,
            })
        else {
            continue;
        };
        let target = walk(func, &uses, head, cell, k);
        if target != head && !enters_loop_body(BlockId(p as u32), target) {
            func.blocks[p].term = Terminator::Br(target);
            edges += 1;
        }
    }
    edges
}

/// Where control lands from `head` while `cell` holds `k`, stopping at
/// the first block whose way out is not decided by that alone.
fn walk(
    func: &Function,
    uses: &HashMap<ValueId, u32>,
    head: BlockId,
    cell: ValueId,
    k: i64,
) -> BlockId {
    let mut cur = head;
    for _ in 0..MAX_STEPS {
        let block = &func.blocks[cur.0 as usize];
        let next = match (&block.insts[..], &block.term) {
            ([], Terminator::Br(t)) => *t,
            (
                [only],
                Terminator::CondBr {
                    cond: Operand::Value(r),
                    then_blk,
                    else_blk,
                },
            ) if only.result == Some(*r) && uses.get(r) == Some(&1) => {
                let Some(taken) = decide(&only.kind, cell, k) else {
                    break;
                };
                if taken { *then_blk } else { *else_blk }
            }
            _ => break,
        };
        if next == cur {
            break;
        }
        cur = next;
    }
    cur
}

/// `icmp pred cell, K'` (or `K', cell`) with `cell = k`.
fn decide(kind: &InstKind, cell: ValueId, k: i64) -> Option<bool> {
    let InstKind::ICmp(pred, a, b) = kind else {
        return None;
    };
    let (l, r) = match (a, b) {
        (Operand::Value(v), Operand::ConstI64(c)) if *v == cell => (k, *c),
        (Operand::ConstI64(c), Operand::Value(v)) if *v == cell => (*c, k),
        _ => return None,
    };
    Some(match pred {
        IPred::Eq => l == r,
        IPred::Ne => l != r,
        IPred::Slt => l < r,
        IPred::Sle => l <= r,
        IPred::Sgt => l > r,
        IPred::Sge => l >= r,
        IPred::Ult => (l as u64) < (r as u64),
        IPred::Uge => (l as u64) >= (r as u64),
    })
}

/// Operand occurrences per value, terminators included.
fn use_counts(func: &Function) -> HashMap<ValueId, u32> {
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *uses.entry(*v).or_insert(0) += 1,
            _ => {}
        }
    }
    uses
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Inst, Type, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
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
    fn test_eq(r: u32, k: i64, then_blk: u32, else_blk: u32) -> (Vec<Inst>, Terminator) {
        (
            vec![inst(
                r,
                InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(0)), Operand::ConstI64(k)),
            )],
            Terminator::CondBr {
                cond: Operand::Value(ValueId(r)),
                then_blk: BlockId(then_blk),
                else_blk: BlockId(else_blk),
            },
        )
    }

    /// b0: s = 0 → b1 | b1 (head): br b2 | b2: s==0 ? b4 : b3 |
    /// b3: s==1 ? b5 : b6 | b4: s = 1 → b1 | b5: ret | b6: ret —
    /// a two-label machine whose goto in b4 re-enters the chain.
    fn machine(extra_use: bool) -> Module {
        let (i2, t2) = test_eq(1, 0, 4, 3);
        let (i3, t3) = test_eq(2, 1, 5, 6);
        let mut b5 = vec![];
        if extra_use {
            b5.push(inst(
                3,
                InstKind::Copy(Type::Bool, Operand::Value(ValueId(2))),
            ));
        }
        let blocks = vec![
            blk(
                0,
                vec![inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(0)))],
                Terminator::Br(BlockId(1)),
            ),
            blk(1, vec![], Terminator::Br(BlockId(2))),
            blk(2, i2, t2),
            blk(3, i3, t3),
            blk(
                4,
                vec![inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(1)))],
                Terminator::Br(BlockId(1)),
            ),
            blk(5, b5, Terminator::Ret(None)),
            blk(6, vec![], Terminator::Ret(None)),
        ];
        let ty = |t| ValueInfo { ty: t, name: None };
        Module {
            funcs: vec![Function {
                name: "m".into(),
                params: vec![],
                ret: Type::Void,
                blocks,
                values: vec![
                    ty(Type::I64),
                    ty(Type::Bool),
                    ty(Type::Bool),
                    ty(Type::Bool),
                ],
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_constant_goto_jumps_to_its_arm() {
        let mut m = machine(false);
        let st = thread_phi_constants(&mut m);
        // b4's goto leaves the loop at b5; b0 would enter the loop
        // body past its header, so it keeps its edge
        assert_eq!((st.edges, st.blocks_swept), (1, 0));
        let f = &m.funcs[0];
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(5))));
    }

    #[test]
    fn a_compare_read_elsewhere_stops_the_walk() {
        let mut m = machine(true);
        assert_eq!(thread_phi_constants(&mut m).edges, 1);
        let f = &m.funcs[0];
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(3))));
    }
}
