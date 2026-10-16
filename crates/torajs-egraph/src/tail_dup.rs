//! Tail duplication of small joins inside loops — the part of LLVM
//! MachineBlockPlacement's tail duplication that the hot-successor
//! chain needs.
//!
//! A block several arms of a loop body rejoin at (the latch after an
//! `if`, the block that adds two arms' results) can fall through from
//! only one of them; every other arm reaches it by a taken branch, on
//! top of the branches inside the arm, and which arm gets the
//! fall-through is a coin toss the chain cannot call (both arms are
//! warm). When the join and the run of small single-successor blocks
//! behind it are a few instructions, each extra warm arm gets its own
//! copy of that tail instead: the copy follows the arm, the arm's
//! branch into the join is gone, and the path runs the same
//! instructions less one. Code grows by the tail's size per extra
//! arm; the tail is cut at a conditional branch, a loop header, a big
//! block, or the size cap, and copies are never themselves copied:
//! duplicating to a fixpoint doubled the copies along every chain of
//! joins (a generator state machine's dispatch took minutes to
//! compile).
//!
//! A copy pays for itself only when it removes a jump and adds none:
//! the arm must reach the join by an unconditional branch (an arm of
//! a conditional branch pays nothing for its edge, whichever side
//! falls through), and the tail must end in the loop's back edge (the
//! header leads the loop's range, so that jump is taken from the
//! original and from the copy alike; a tail ending anywhere else may
//! fall through from the original, and the copy's jump would be new:
//! csv-trim's trim loops lost an instruction a part that way).
//!
//! An arm every path to which runs a call, ends the function, or
//! takes an edge the lowerer's guards mark rare is cold: the chain
//! lays it after the warm path anyway, so a copy there would save a
//! branch nothing takes, and the allocator judges call crossing on a
//! value's positional hull, so a copy placed past a call would push
//! every value live through the tail into the callee-saved pool.
//!
//! A copy defines fresh value ids for the values that live only
//! inside the tail (every def and every read in the tail, defined
//! before read along it); a value read outside the tail keeps its id
//! in the copy, as a cell with several defs (the shape φ destruction
//! leaves). Keeping the ids throughout made every tail value multi-
//! use: the single-use gates of the compare / select fusions failed
//! (regex-wireback's `cmp; cinc` came apart into `cset; cmp; csel`),
//! and each value's hull stretched over every copy. Every path still
//! executes the tail once. A block that allocates a frame slot or
//! calls is left alone: an alloca names one slot, and a call's arm is
//! cold anyway.
//!
//! `TORAJS_TAIL_DUP_OFF=1` skips (bisect gate).

use crate::block_chain::{ChainFacts, ColdEdges};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

use torajs_core::ssa::{
    Block, BlockId, Function, InstKind, Terminator, ValueId, rewrite_value_operands,
};

mod after;
mod joins;
mod sites;
pub(crate) use after::{add_to_loops, idoms_after};
use joins::joins_to_copy;
use sites::Sites;

/// Largest block in a duplicated tail, in instructions.
const MAX_INSTS: usize = 6;
/// Largest tail duplicated, in instructions.
const MAX_TAIL_INSTS: usize = 10;

/// What [`duplicate_small_joins`] changed.
#[derive(Default)]
pub(crate) struct Copied {
    pub(crate) joins: Vec<CopiedJoin>,
    /// The guards' cold edges as read before copying, when no copy
    /// writes a value an original writes too: every value a copy
    /// writes is then read only inside it, and every guard reads as
    /// it did.
    pub(crate) cold_edges: Option<ColdEdges>,
}

/// One join whose tail was copied.
pub(crate) struct CopiedJoin {
    /// The original tail, join first.
    pub(crate) tail: Vec<u32>,
    /// The header of the loop the tail branches back to.
    pub(crate) header: u32,
    /// Per copy, the arm that now jumps to it and its first block; its
    /// blocks follow in tail order.
    pub(crate) copies: Vec<(u32, u32)>,
}

impl Copied {
    /// How many blocks were copied.
    pub(crate) fn blocks(&self) -> u32 {
        (self.joins.iter())
            .map(|j| (j.tail.len() * j.copies.len()) as u32)
            .sum()
    }
}

/// Give every extra warm in-loop predecessor of a small join its own
/// copy of the join and the small blocks behind it; `dom` and `la` are
/// the function's as it stands.
pub(crate) fn duplicate_small_joins(
    func: &mut Function,
    dom: &DominatorTree,
    la: &LoopAnalysis,
) -> Copied {
    if off() || !any_candidate(func) || la.loops().is_empty() {
        return Copied::default();
    }
    let (joins, facts) = joins_to_copy(func, dom, la);
    if joins.is_empty() {
        return Copied::default();
    }
    let sites = Sites::of(func, joins.iter().map(|(_, tail, _, _)| tail.as_slice()));
    let copied: usize = (joins.iter())
        .map(|(_, tail, _, arms)| tail.len() * arms.len())
        .sum();
    func.blocks.reserve(copied);
    let mut renamed = true;
    let mut out = Vec::with_capacity(joins.len());
    for (id, tail, header, arms) in joins {
        let local = sites.local_to(func, &tail);
        let writes = tail.iter().flat_map(|&b| &func.blocks[b as usize].insts);
        renamed &= (writes.filter_map(|i| i.result))
            .all(|r| local.binary_search_by_key(&r.0, |v| v.0).is_ok());
        let mut copies = Vec::with_capacity(arms.len());
        func.values.reserve(local.len() * arms.len());
        for p in arms {
            let first = copy_tail(func, &tail, &local);
            retarget(&mut func.blocks[p as usize].term, BlockId(id), first);
            copies.push((p, first.0));
        }
        out.push(CopiedJoin {
            tail,
            header,
            copies,
        });
    }
    let cold_edges = facts.filter(|_| renamed).map(ChainFacts::into_cold_edges);
    Copied {
        joins: out,
        cold_edges,
    }
}

/// `TORAJS_TAIL_DUP_OFF=1` (bisect gate), read once per process: the
/// pass asks for every function.
fn off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("TORAJS_TAIL_DUP_OFF").is_some_and(|v| v == "1"))
}

/// Whether some block could head a copied tail: it is small, and of
/// two or more predecessors one jumps to it. Most functions have none,
/// and skip the dominator and loop analyses.
fn any_candidate(func: &Function) -> bool {
    let mut preds = vec![(0u32, false); func.blocks.len()];
    for block in &func.blocks {
        let jump = matches!(block.term, Terminator::Br(_));
        for t in targets(&block.term) {
            let p = &mut preds[t as usize];
            p.0 += 1;
            p.1 |= jump;
        }
    }
    func.blocks
        .iter()
        .zip(&preds)
        .any(|(b, &(n, jump))| n >= 2 && jump && duplicable(b))
}

/// Append a copy of the blocks of `tail`, chained in order, in which
/// every value of `local` gets a fresh id; answers the copy's first
/// block.
fn copy_tail(func: &mut Function, tail: &[u32], local: &[ValueId]) -> BlockId {
    let first = BlockId(func.blocks.len() as u32);
    // `local` is in id order, and its i-th value becomes the i-th
    // fresh one
    let fresh = func.values.len() as u32;
    for &v in local {
        func.values.push(func.values[v.0 as usize].clone());
    }
    let renamed = |v: &ValueId| {
        (local.binary_search_by_key(&v.0, |l| l.0)).map_or(*v, |i| ValueId(fresh + i as u32))
    };
    for (k, &t) in tail.iter().enumerate() {
        let copy = BlockId(func.blocks.len() as u32);
        let mut term = func.blocks[t as usize].term.clone();
        if k + 1 < tail.len() {
            term = Terminator::Br(BlockId(copy.0 + 1));
        }
        let insts = func.blocks[t as usize]
            .insts
            .iter()
            .map(|inst| {
                let mut inst = inst.clone();
                if let Some(r) = inst.result.as_mut() {
                    *r = renamed(r);
                }
                rewrite_value_operands(&mut inst.kind, |v| renamed(&v));
                inst
            })
            .collect();
        func.blocks.push(Block {
            id: copy,
            insts,
            term,
        });
    }
    first
}

/// A few instructions, none of them a frame slot or a call (an `any`
/// drop may call), ending in an unconditional branch.
fn duplicable(block: &Block) -> bool {
    matches!(block.term, Terminator::Br(_))
        && !block.insts.is_empty()
        && block.insts.len() <= MAX_INSTS
        && !block.insts.iter().any(|i| {
            matches!(
                i.kind,
                InstKind::Alloca(_)
                    | InstKind::AllocaBytes(_)
                    | InstKind::Call(..)
                    | InstKind::CallIndirect(..)
                    | InstKind::RcDecAny(_)
            )
        })
}

/// The blocks `term` branches to, a conditional branch's two arms both
/// even when they are one block. The pass asks for every block of
/// every function, so nothing is allocated.
fn targets(term: &Terminator) -> impl Iterator<Item = u32> {
    let (a, b) = match term {
        Terminator::Br(t) => (Some(t.0), None),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => (Some(then_blk.0), Some(else_blk.0)),
        Terminator::Ret(_) | Terminator::Unreachable => (None, None),
    };
    a.into_iter().chain(b)
}

/// Point every edge of `term` that goes to `from` at `to`.
fn retarget(term: &mut Terminator, from: BlockId, to: BlockId) {
    match term {
        Terminator::Br(t) if *t == from => *t = to,
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            if *then_blk == from {
                *then_blk = to;
            }
            if *else_blk == from {
                *else_blk = to;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, IPred, Inst, Operand, Type, ValueInfo};

    fn dup(f: &mut Function) -> u32 {
        let dom = DominatorTree::compute(f);
        let la = LoopAnalysis::compute(f, &dom);
        duplicate_small_joins(f, &dom, &la).blocks()
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn add1(r: u32) -> Inst {
        inst(
            r,
            InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(r)), Operand::ConstI64(1)),
        )
    }

    fn cbr(t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::Value(ValueId(0)),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    fn func(values: usize, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                values
            ],
            blocks,
            current_origin: None,
        }
    }

    /// b0 → b1(hdr: ? b2 : b6) ; b2: ? b3 : b4 ; b3 → b5 ; b4 → b5 ;
    /// b5 (latch: add) → b1 ; b6: ret
    fn diamond_before_latch(latch: Vec<Inst>) -> Function {
        func(
            2,
            vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(1, vec![], cbr(2, 6)),
                block(2, vec![], cbr(3, 4)),
                block(3, vec![], Terminator::Br(BlockId(5))),
                block(4, vec![], Terminator::Br(BlockId(5))),
                block(5, latch, Terminator::Br(BlockId(1))),
                block(6, vec![], Terminator::Ret(None)),
            ],
        )
    }

    #[test]
    fn the_second_arm_gets_its_own_copy_of_the_latch() {
        let mut f = diamond_before_latch(vec![add1(1)]);
        assert_eq!(dup(&mut f), 1);
        assert_eq!(f.blocks.len(), 8);
        assert!(matches!(f.blocks[3].term, Terminator::Br(BlockId(5))));
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(7))));
        assert_eq!(f.blocks[7].insts.len(), 1);
        assert!(matches!(f.blocks[7].term, Terminator::Br(BlockId(1))));
        // the cell carried around the loop keeps its id in the copy
        assert_eq!(f.blocks[7].insts[0].result, Some(ValueId(1)));
    }

    #[test]
    fn a_join_past_the_size_cap_or_with_a_call_stays_shared() {
        let mut f = diamond_before_latch(vec![add1(1); MAX_INSTS + 1]);
        assert_eq!(dup(&mut f), 0);
        let call = Inst {
            result: None,
            kind: InstKind::Call(torajs_core::ssa::FuncId(0), vec![]),
            origin: None,
        };
        let mut f = diamond_before_latch(vec![call]);
        assert_eq!(dup(&mut f), 0);
        let drop = inst(1, InstKind::RcDecAny(Operand::Value(ValueId(1))));
        let mut f = diamond_before_latch(vec![Inst {
            result: None,
            ..drop
        }]);
        assert_eq!(dup(&mut f), 0);
    }

    /// A join that branches to the latch: the latch is part of its
    /// tail, so the extra arm gets both.
    ///
    /// b0 → b1(hdr: ? b2 : b7) ; b2: ? b3 : b4 ; b3 → b5 ; b4 → b5 ;
    /// b5 (join: add) → b6 ; b6 (latch: add) → b1 ; b7: ret
    #[test]
    fn a_copied_join_gets_its_own_copy_of_the_latch() {
        let mut f = func(
            2,
            vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(1, vec![], cbr(2, 7)),
                block(2, vec![], cbr(3, 4)),
                block(3, vec![], Terminator::Br(BlockId(5))),
                block(4, vec![], Terminator::Br(BlockId(5))),
                block(5, vec![add1(1)], Terminator::Br(BlockId(6))),
                block(6, vec![add1(1)], Terminator::Br(BlockId(1))),
                block(7, vec![], Terminator::Ret(None)),
            ],
        );
        assert_eq!(dup(&mut f), 2);
        // b4's copy of the join (b8) got its own latch copy (b9)
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(8))));
        assert!(matches!(f.blocks[8].term, Terminator::Br(BlockId(9))));
        assert!(matches!(f.blocks[9].term, Terminator::Br(BlockId(1))));
    }

    /// The loop header is a join of the entry and the latch; it is
    /// never duplicated.
    #[test]
    fn the_header_is_not_a_join_to_duplicate() {
        let mut f = diamond_before_latch(vec![]);
        f.blocks[1].insts.push(add1(1));
        assert_eq!(dup(&mut f), 0);
    }

    /// The counting-loop tail: `%2 = icmp ne %1, 0 ; %3 = add %4, 1 ;
    /// %4 = select %2, %3, %4`. The compare and the increment are read
    /// only by the select, so the copy defines them afresh; the
    /// select's result is the carried counter and keeps its id.
    #[test]
    fn a_copy_renames_the_values_that_live_only_in_the_tail() {
        let latch = vec![
            inst(
                2,
                InstKind::ICmp(IPred::Ne, Operand::Value(ValueId(1)), Operand::ConstI64(0)),
            ),
            inst(
                3,
                InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(4)), Operand::ConstI64(1)),
            ),
            inst(
                4,
                InstKind::Select(
                    Type::I64,
                    Operand::Value(ValueId(2)),
                    Operand::Value(ValueId(3)),
                    Operand::Value(ValueId(4)),
                ),
            ),
        ];
        let mut f = diamond_before_latch(latch);
        f.values.resize(
            5,
            ValueInfo {
                ty: Type::I64,
                name: None,
            },
        );
        assert_eq!(dup(&mut f), 1);
        assert_eq!(f.values.len(), 7, "two fresh values for the copy");
        let copy = &f.blocks[7].insts;
        assert_eq!(copy[0].result, Some(ValueId(5)));
        assert_eq!(copy[1].result, Some(ValueId(6)));
        assert_eq!(copy[2].result, Some(ValueId(4)), "the counter keeps its id");
        assert!(matches!(
            copy[1].kind,
            InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(4)), _)
        ));
        assert!(matches!(
            copy[2].kind,
            InstKind::Select(_, Operand::Value(ValueId(5)), Operand::Value(ValueId(6)), _)
        ));
        // the original tail is untouched
        assert_eq!(f.blocks[5].insts[0].result, Some(ValueId(2)));
    }

    /// An arm behind a call is cold: it keeps branching into the
    /// shared join, which leaves the warm arm free to take a copy.
    ///
    /// b0 → b1(hdr: ? b2 : b6) ; b2: ? b3 : b4 ; b3 → b5 ; b4 (call) → b5 ;
    /// b5 (latch: add) → b1 ; b6: ret
    #[test]
    fn an_arm_behind_a_call_keeps_the_shared_tail() {
        let mut f = diamond_before_latch(vec![add1(1)]);
        f.blocks[4].insts.push(Inst {
            result: None,
            kind: InstKind::Call(torajs_core::ssa::FuncId(0), vec![]),
            origin: None,
        });
        assert_eq!(dup(&mut f), 1);
        assert!(matches!(f.blocks[3].term, Terminator::Br(BlockId(7))));
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(5))));
    }

    /// A tail block entered from outside the tail: a path through that
    /// edge reads `%2` without running the join that writes it, so the
    /// copy must write the same `%2`, not a fresh one.
    ///
    /// b0 → b1(hdr: ? b2 : b9) ; b2: ? b3 : b7 ; b3 → b5 ; b7: ? b4 : b8 ;
    /// b4 → b5 ; b5 (join: %2 = %1 + 1) → b6 ; b8 → b6 ;
    /// b6 (latch: %3 = %2 + 1) → b1 ; b9: ret
    #[test]
    fn a_value_read_past_a_second_entry_keeps_its_id() {
        let read = |r: u32, v: u32| {
            inst(
                r,
                InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(v)), Operand::ConstI64(1)),
            )
        };
        let mut f = func(
            4,
            vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(1, vec![], cbr(2, 9)),
                block(2, vec![], cbr(3, 7)),
                block(3, vec![], Terminator::Br(BlockId(5))),
                block(4, vec![], Terminator::Br(BlockId(5))),
                block(5, vec![read(2, 1)], Terminator::Br(BlockId(6))),
                block(6, vec![read(3, 2)], Terminator::Br(BlockId(1))),
                block(7, vec![], cbr(4, 8)),
                block(8, vec![], Terminator::Br(BlockId(6))),
                block(9, vec![], Terminator::Ret(None)),
            ],
        );
        assert_eq!(dup(&mut f), 3);
        // b4's copy of the join (b10, b11)
        assert!(matches!(f.blocks[4].term, Terminator::Br(BlockId(10))));
        assert_eq!(f.blocks[10].insts[0].result, Some(ValueId(2)));
        assert!(matches!(
            f.blocks[11].insts[0].kind,
            InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(2)), _)
        ));
        assert_ne!(f.blocks[11].insts[0].result, Some(ValueId(3)));
    }

    /// Arms that reach the join by a conditional branch pay nothing
    /// for the edge, so there is nothing for a copy to save.
    ///
    /// b0 → b1(hdr: ? b2 : b6) ; b2: ? b5 : b3 ; b3: ? b5 : b4 ; b4 → b6 ;
    /// b5 (latch: add) → b1 ; b6: ret
    #[test]
    fn conditional_arms_get_no_copy() {
        let mut f = diamond_before_latch(vec![add1(1)]);
        f.blocks[2].term = cbr(5, 3);
        f.blocks[3].term = cbr(5, 4);
        f.blocks[4].term = Terminator::Br(BlockId(6));
        assert_eq!(dup(&mut f), 0);
    }

    /// A tail that ends anywhere but the back edge may fall through
    /// into its successor; the copy's jump there would be new.
    ///
    /// b0 → b1(hdr: ? b2 : b7) ; b2: ? b3 : b4 ; b3 → b5 ; b4 → b5 ;
    /// b5 (join: add) → b6 ; b6 (latch): ? b1 : b7 ; b7: ret
    #[test]
    fn a_tail_short_of_the_back_edge_gets_no_copy() {
        let mut f = func(
            2,
            vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(1, vec![], cbr(2, 7)),
                block(2, vec![], cbr(3, 4)),
                block(3, vec![], Terminator::Br(BlockId(5))),
                block(4, vec![], Terminator::Br(BlockId(5))),
                block(5, vec![add1(1)], Terminator::Br(BlockId(6))),
                block(6, vec![], cbr(1, 7)),
                block(7, vec![], Terminator::Ret(None)),
            ],
        );
        assert_eq!(dup(&mut f), 0);
    }
}
