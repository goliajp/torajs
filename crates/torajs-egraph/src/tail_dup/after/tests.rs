//! The dominators and loops read off the old ones against the ones
//! computed afresh, on random graphs that tail duplication changes.

use super::*;
use crate::block_chain::ChainFacts;
use crate::tail_dup::duplicate_small_joins;

use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    BinOp, Block, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
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

/// Blocks of zero to two `%v = %v + 1` (a cell carried around) or
/// `%t = %v + 1` (a value no other block reads), ending in a jump, a
/// branch, a return or a trap, to random blocks.
fn random_function(rng: &mut Rng) -> Function {
    let n = 4 + rng.below(14) as u32;
    let mut blocks = Vec::new();
    let mut values = VALUES;
    for id in 0..n {
        let carried = rng.below(2) == 0;
        let insts = (0..rng.below(3))
            .map(|_| {
                let v = ValueId(rng.below(u64::from(VALUES)) as u32);
                let r = match carried {
                    true => v,
                    false => {
                        values += 1;
                        ValueId(values - 1)
                    }
                };
                Inst {
                    result: Some(r),
                    kind: InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1)),
                    origin: None,
                }
            })
            .collect();
        let kind = rng.below(20);
        let (t, e) = (
            rng.below(u64::from(n)) as u32,
            rng.below(u64::from(n)) as u32,
        );
        let term = match kind {
            0..=10 => Terminator::Br(BlockId(t)),
            11..=17 => Terminator::CondBr {
                cond: Operand::Value(ValueId(0)),
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
    crate::branch_fold::sweep_unreachable_blocks(&mut f);
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

#[test]
fn read_off_the_old_tree_and_loops_matches_computing_them_afresh() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut checked, mut fell_back, mut kept_edges) = (0, 0, 0);
    for _ in 0..100_000 {
        let mut f = random_function(&mut rng);
        let dom = DominatorTree::compute(&f);
        let la = LoopAnalysis::compute(&f, &dom);
        let copied = duplicate_small_joins(&mut f, &dom, &la);
        if copied.joins.is_empty() {
            continue;
        }
        let fresh = DominatorTree::compute(&f);
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
        if let Some(edges) = copied.cold_edges {
            let now = ChainFacts::new(&f, DomView::whole(&fresh)).into_cold_edges();
            assert_eq!(edges, now);
            kept_edges += 1;
        }
    }
    // the graphs must exercise the update, not skip it
    assert!(checked > 1000, "checked {checked}, fell back {fell_back}");
    assert!(kept_edges > 200, "kept the cold edges {kept_edges} times");
}
