//! Static block frequencies for the heuristics that weigh code by how
//! often it runs: spill weights and the cost of saving a register
//! around a call.
//!
//! `freq(b) = ENTRY_FREQ × LOOP_SCALE^min(depth(b), NEST_CAP) × Π p`,
//! where `depth` counts the natural loops holding `b` (back edges by
//! the dominator tree) and the product runs over the cold arms of
//! [`super::branch_prob`] whose target dominates `b`. A cold arm's
//! target counts only when that branch is its sole predecessor, so the
//! join after the arm runs at full frequency again.
//!
//! A loop level stands for 16 iterations, the classic block-frequency
//! figure. Branches no rule reads split nothing; see `branch_prob` for
//! why an even split is worse than none here.

use crate::ssa::branch_prob::cold_arms;
use crate::ssa::dominator::DominatorTree;
use crate::ssa::loop_analysis::LoopAnalysis;
use crate::ssa::{BlockId, Function, Terminator};

/// Frequency of the entry block.
pub const ENTRY_FREQ: u64 = 1024;
/// Iterations per entry a loop is credited with.
pub const LOOP_SCALE: u64 = 16;
/// Deepest nest whose extra level still raises the estimate.
const NEST_CAP: u32 = 4;

/// Estimated executions of each block per function entry, in units of
/// `1 / ENTRY_FREQ`, indexed by `BlockId.0`. Unreachable blocks get 0,
/// reachable ones at least 1.
pub fn block_freqs(func: &Function) -> Vec<u64> {
    let n = func
        .blocks
        .iter()
        .map(|b| b.id.0 as usize + 1)
        .max()
        .unwrap_or(0);
    let mut out = vec![0u64; n];
    if func.blocks.is_empty() {
        return out;
    }
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);

    let mut preds = vec![0u32; n];
    for b in &func.blocks {
        match &b.term {
            Terminator::Br(t) => preds[t.0 as usize] += 1,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                preds[then_blk.0 as usize] += 1;
                if else_blk != then_blk {
                    preds[else_blk.0 as usize] += 1;
                }
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    let mut arm_odds = vec![1.0f64; n];
    for (_, cold, p) in cold_arms(func) {
        if preds[cold.0 as usize] == 1 {
            arm_odds[cold.0 as usize] *= p;
        }
    }

    // walk the dominator tree carrying the product of cold-arm odds
    let entry = func.blocks[0].id;
    let mut stack: Vec<(BlockId, f64)> = vec![(entry, arm_odds[entry.0 as usize])];
    while let Some((b, odds)) = stack.pop() {
        let depth = loops.nest_depth(b).min(NEST_CAP);
        let f = (ENTRY_FREQ * LOOP_SCALE.pow(depth)) as f64 * odds;
        out[b.0 as usize] = (f.round() as u64).max(1);
        for &c in dom.children(b) {
            stack.push((c, odds * arm_odds[c.0 as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{
        BinOp, Block, FuncId, IPred, Inst, InstKind, Operand, THROW_ACTIVE_SYM, Type, ValueId,
    };

    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "test".to_string(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![],
            current_origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn call() -> Inst {
        Inst {
            result: None,
            kind: InstKind::Call(FuncId(0), vec![]),
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn cond_br(cond: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(cond),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    fn br(to: u32) -> Terminator {
        Terminator::Br(BlockId(to))
    }

    const RET: Terminator = Terminator::Ret(None);

    #[test]
    fn a_loop_with_one_exit_runs_sixteen_times() {
        let f = func(vec![
            block(0, vec![], br(1)),
            block(1, vec![], cond_br(100, 2, 3)),
            block(2, vec![], br(1)),
            block(3, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 16 * 1024, 16 * 1024, 1024]);
    }

    #[test]
    fn nested_loops_multiply() {
        let f = func(vec![
            block(0, vec![], br(1)),
            block(1, vec![], cond_br(100, 2, 5)),
            block(2, vec![], cond_br(101, 3, 4)),
            block(3, vec![], br(2)),
            block(4, vec![], br(1)),
            block(5, vec![], RET),
        ]);
        assert_eq!(
            block_freqs(&f),
            vec![1024, 16 * 1024, 256 * 1024, 256 * 1024, 16 * 1024, 1024]
        );
    }

    #[test]
    fn the_throw_arm_of_a_probe_is_cold() {
        let probe = vec![
            inst(0, InstKind::GlobalRef(THROW_ACTIVE_SYM.to_string())),
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(2, InstKind::ICmp(IPred::Ne, v(1), Operand::ConstI64(0))),
        ];
        let f = func(vec![
            block(0, probe, cond_br(2, 1, 2)),
            block(1, vec![], RET),
            block(2, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1, 1024]);
    }

    #[test]
    fn a_header_flag_guard_predicts_its_calling_arm_cold() {
        let guard = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(1 << 49)),
            ),
            inst(3, InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0))),
        ];
        let f = func(vec![
            block(0, guard, cond_br(3, 2, 1)),
            block(1, vec![], br(3)),
            block(2, vec![call()], br(3)),
            block(3, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 16, 1024]);
    }

    #[test]
    fn a_length_guard_predicts_its_grow_arm_cold() {
        let guard = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 8)),
            inst(2, InstKind::ICmp(IPred::Ult, v(9), v(1))),
        ];
        let f = func(vec![
            block(0, guard, cond_br(2, 1, 2)),
            block(1, vec![], br(3)),
            block(2, vec![call()], br(3)),
            block(3, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 16, 1024]);
    }

    #[test]
    fn a_user_branch_with_a_call_is_not_cold() {
        // `if (q.length > 16) q.shift()` is taken on almost every
        // iteration; a call alone says nothing
        let f = func(vec![
            block(0, vec![], cond_br(100, 1, 2)),
            block(1, vec![call()], br(2)),
            block(2, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 1024]);
    }

    #[test]
    fn both_copies_of_a_versioned_loop_stay_hot() {
        // an unread branch picks one of two identical loops; halving
        // each would underprice the copy that runs
        let f = func(vec![
            block(0, vec![], cond_br(100, 1, 3)),
            block(1, vec![], cond_br(101, 2, 5)),
            block(2, vec![], br(1)),
            block(3, vec![], cond_br(102, 4, 5)),
            block(4, vec![], br(3)),
            block(5, vec![], RET),
        ]);
        let freqs = block_freqs(&f);
        assert_eq!(freqs[2], 16 * 1024);
        assert_eq!(freqs[4], 16 * 1024);
    }

    #[test]
    fn a_cold_arm_inside_a_loop_is_scaled_by_both() {
        let guard = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(1 << 49)),
            ),
            inst(3, InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0))),
        ];
        // header 1 exits to 5; body 2 guards a slow call in 3; latch 4
        let f = func(vec![
            block(0, vec![], br(1)),
            block(1, vec![], cond_br(100, 2, 5)),
            block(2, guard, cond_br(3, 3, 4)),
            block(3, vec![call()], br(4)),
            block(4, vec![], br(1)),
            block(5, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 16384, 16384, 256, 16384, 1024]);
    }

    #[test]
    fn a_loop_exit_leaves_the_loop_level() {
        // header 1 tests, body 2 may break out to 4, latch 3
        let f = func(vec![
            block(0, vec![], br(1)),
            block(1, vec![], cond_br(100, 2, 4)),
            block(2, vec![], cond_br(101, 4, 3)),
            block(3, vec![], br(1)),
            block(4, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 16384, 16384, 16384, 1024]);
    }

    #[test]
    fn an_unreachable_block_has_no_frequency() {
        let f = func(vec![block(0, vec![], RET), block(1, vec![], RET)]);
        assert_eq!(block_freqs(&f), vec![1024, 0]);
    }
}
