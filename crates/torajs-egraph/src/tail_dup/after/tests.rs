//! The dominators, loops and cold edges read off the old ones against
//! the ones computed afresh, on random graphs that tail duplication
//! changes.

use super::*;
use crate::block_chain::{ChainFacts, ColdEdges};
use crate::tail_dup::duplicate_small_joins;

use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    BinOp, Block, FuncId, IPred, Inst, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, Type,
    ValueId, ValueInfo,
};

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

const VALUES: u32 = 4;
/// Guards per function, each a throw probe or a null test.
const GUARDS: u32 = 3;

fn inst(r: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(r)),
        kind,
        origin: None,
    }
}

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

/// The defs of guard `k`, whose compare is the last: a throw probe
/// `icmp ne (load (global_ref THROW_ACTIVE)), 0`, or a null test
/// `icmp eq (ptrtoint %1), 0`, which predicts the calling arm cold.
fn guard(k: u32, probe: bool) -> Vec<Inst> {
    let r = VALUES + 3 * k;
    if probe {
        vec![
            inst(r, InstKind::GlobalRef(THROW_ACTIVE_SYM.to_string())),
            inst(r + 1, InstKind::Load(Type::I64, v(r), 0)),
            inst(
                r + 2,
                InstKind::ICmp(IPred::Ne, v(r + 1), Operand::ConstI64(0)),
            ),
        ]
    } else {
        vec![
            inst(r + 1, InstKind::PtrToInt(v(1))),
            inst(
                r + 2,
                InstKind::ICmp(IPred::Eq, v(r + 1), Operand::ConstI64(0)),
            ),
        ]
    }
}

/// Blocks of zero to two `%v = %v + 1` (a cell carried around) or
/// `%t = %v + 1` (a value no other block reads), some with a call,
/// ending in a jump, a branch, a return or a trap, to random blocks.
/// Each guard's defs land in random blocks, and a branch tests `%0`
/// or a guard's compare.
fn random_function(rng: &mut Rng) -> Function {
    let n = 4 + rng.below(14) as u32;
    let mut blocks = Vec::new();
    let mut values = VALUES + 3 * GUARDS;
    for id in 0..n {
        let carried = rng.below(2) == 0;
        let mut insts: Vec<Inst> = (0..rng.below(3))
            .map(|_| {
                let v = ValueId(rng.below(u64::from(VALUES)) as u32);
                let r = match carried {
                    true => v,
                    false => {
                        values += 1;
                        ValueId(values - 1)
                    }
                };
                inst(
                    r.0,
                    InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1)),
                )
            })
            .collect();
        if rng.below(6) == 0 {
            insts.push(Inst {
                result: None,
                kind: InstKind::Call(FuncId(0), vec![]),
                origin: None,
            });
        }
        let (t, e) = (
            rng.below(u64::from(n)) as u32,
            rng.below(u64::from(n)) as u32,
        );
        let cond = match rng.below(u64::from(GUARDS) + 1) as u32 {
            0 => v(0),
            k => v(VALUES + 3 * (k - 1) + 2),
        };
        let term = match rng.below(20) {
            0..=10 => Terminator::Br(BlockId(t)),
            11..=17 => Terminator::CondBr {
                cond,
                then_blk: BlockId(t),
                else_blk: BlockId(e),
            },
            18 => Terminator::Unreachable,
            _ => Terminator::Ret(None),
        };
        blocks.push(Block {
            id: BlockId(id),
            insts,
            term,
        });
    }
    for k in 0..GUARDS {
        for def in guard(k, rng.below(2) == 0) {
            blocks[rng.below(u64::from(n)) as usize].insts.push(def);
        }
    }
    let mut f = Function {
        name: "f".into(),
        params: vec![],
        ret: Type::Void,
        values: vec![
            ValueInfo {
                ty: Type::I64,
                name: None
            };
            values as usize
        ],
        blocks,
        current_origin: None,
    };
    // one graph in four keeps its unreachable blocks
    if rng.below(4) != 0 {
        crate::branch_fold::sweep_unreachable_blocks(&mut f);
    }
    f
}

fn loop_sets(la: &LoopAnalysis) -> Vec<(BlockSet, u32, Vec<u32>)> {
    (la.loops().iter())
        .map(|l| {
            let body = BlockSet::of(l.body.iter().map(|b| b.0));
            let latches = l.back_edge_sources.iter().map(|b| b.0).collect();
            (body, l.header.0, latches)
        })
        .collect()
}

fn cold_edges(f: &Function, dom: &DominatorTree) -> ColdEdges {
    ChainFacts::new(f, DomView::whole(dom)).into_cold_edges()
}

/// Whether one of `edges` leaves a branch on a guard's compare.
fn on_a_guard(f: &Function, edges: &ColdEdges) -> bool {
    edges.iter().any(|(b, _)| {
        matches!(&f.blocks[b as usize].term,
            Terminator::CondBr { cond: Operand::Value(c), .. } if c.0 >= VALUES)
    })
}

#[test]
fn read_off_the_old_tree_and_loops_matches_computing_them_afresh() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut checked, mut fell_back) = (0, 0);
    // copied graphs; those with cold edges; those with one on a guard and the
    // cold edges kept; those whose cold edges copying changed; those
    // whose cold edges were not kept
    let (mut copies, mut cold, mut kept_guarded, mut changed, mut dropped) = (0, 0, 0, 0, 0);
    for _ in 0..300_000 {
        let mut f = random_function(&mut rng);
        let dom = DominatorTree::compute(&f);
        let la = LoopAnalysis::compute(&f, &dom);
        let before = cold_edges(&f, &dom);
        let copied = duplicate_small_joins(&mut f, &dom, &la);
        if copied.joins.is_empty() {
            continue;
        }
        copies += 1;
        let fresh = DominatorTree::compute(&f);
        let now = cold_edges(&f, &fresh);
        cold += usize::from(before.iter().next().is_some());
        changed += usize::from(before != now);
        match &copied.cold_edges {
            Some(edges) => {
                assert_eq!(*edges, now, "kept cold edges went stale");
                kept_guarded += usize::from(on_a_guard(&f, edges));
            }
            None => dropped += 1,
        }
        let Some(idoms) = idoms_after(&f, &dom, &copied) else {
            fell_back += 1;
            continue;
        };
        checked += 1;
        let view = DomView::patched(&dom, &idoms);
        let n = f.blocks.len() as u32;
        for a in (0..n).map(BlockId) {
            assert_eq!(view.immediate_dominator(a), fresh.immediate_dominator(a));
            for b in (0..n).map(BlockId) {
                assert_eq!(view.dominates(a, b), fresh.dominates(a, b), "{a:?} {b:?}");
            }
        }
        let mut loops = loop_sets(&la);
        add_to_loops(&mut loops, &copied);
        assert_eq!(loops, loop_sets(&LoopAnalysis::compute(&f, &fresh)));
    }
    eprintln!(
        "copied {copies}: checked {checked} fell back {fell_back} cold {cold} \
         kept on a guard {kept_guarded} changed {changed} dropped {dropped}"
    );
    // the graphs must exercise the updates, not skip them: cold edges
    // the guards read, kept, and copies that change them
    assert!(checked > 1500, "checked {checked}, fell back {fell_back}");
    assert!(
        cold * 2 > copies,
        "{cold} of {copies} copied graphs have cold edges"
    );
    assert!(
        kept_guarded > 500,
        "kept guard cold edges {kept_guarded} times"
    );
    assert!(
        changed > 1000,
        "copying changed the cold edges {changed} times"
    );
    assert!(dropped >= changed, "dropped {dropped}, changed {changed}");
}

/// A tail that writes a guard's compare the header branches on: the
/// copy writes it too, the compare no longer has one def, and the
/// header's cold edge is gone, so the edges read before copying are
/// not kept.
///
/// b0 → b1(hdr: %6 ? b6 : b2) ; b2: ? b3 : b4 ; b3 → b5 ; b4 → b5 ;
/// b5 (latch: the probe into %6) → b1 ; b6: ret
#[test]
fn a_copy_that_writes_a_value_read_outside_drops_the_cold_edges() {
    let blocks = vec![
        Block {
            id: BlockId(0),
            insts: vec![],
            term: Terminator::Br(BlockId(1)),
        },
        Block {
            id: BlockId(1),
            insts: vec![],
            term: Terminator::CondBr {
                cond: v(VALUES + 2),
                then_blk: BlockId(6),
                else_blk: BlockId(2),
            },
        },
        Block {
            id: BlockId(2),
            insts: vec![],
            term: Terminator::CondBr {
                cond: v(0),
                then_blk: BlockId(3),
                else_blk: BlockId(4),
            },
        },
        Block {
            id: BlockId(3),
            insts: vec![],
            term: Terminator::Br(BlockId(5)),
        },
        Block {
            id: BlockId(4),
            insts: vec![],
            term: Terminator::Br(BlockId(5)),
        },
        Block {
            id: BlockId(5),
            insts: guard(0, true),
            term: Terminator::Br(BlockId(1)),
        },
        Block {
            id: BlockId(6),
            insts: vec![],
            term: Terminator::Ret(None),
        },
    ];
    let mut f = Function {
        name: "f".into(),
        params: vec![],
        ret: Type::Void,
        values: vec![
            ValueInfo {
                ty: Type::I64,
                name: None
            };
            (VALUES + 3) as usize
        ],
        blocks,
        current_origin: None,
    };
    let dom = DominatorTree::compute(&f);
    let la = LoopAnalysis::compute(&f, &dom);
    let before = cold_edges(&f, &dom);
    assert!(before.contains(1, 6), "the probe's throw arm");
    let copied = duplicate_small_joins(&mut f, &dom, &la);
    assert_eq!(copied.blocks(), 1);
    assert!(copied.cold_edges.is_none());
    let now = cold_edges(&f, &DominatorTree::compute(&f));
    assert!(now.iter().next().is_none());
}
