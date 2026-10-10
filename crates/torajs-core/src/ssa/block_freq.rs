//! Static block frequencies for the heuristics that weigh code by how
//! often it runs: spill weights and the cost of saving a register
//! around a call.
//!
//! `freq(b) = ENTRY_FREQ × LOOP_SCALE^min(depth(b), NEST_CAP) × p`,
//! where `depth` counts the natural loops holding `b` (back edges by
//! the dominator tree) and `p` is the share of runs that reach `b`:
//! the sum over its incoming edges of the predecessor's share times
//! the edge's odds (a cold arm of [`super::branch_prob`], else 1),
//! capped by its immediate dominator's share (Wu & Larus, MICRO 1994,
//! without the loop scaling). The join after a cold arm and a warm one
//! runs at full frequency again; a join every path into which comes
//! through a cold arm (the one slow call several guards share) stays
//! cold.
//!
//! A loop level stands for 16 iterations, the classic block-frequency
//! figure. Branches no rule reads split nothing; see `branch_prob` for
//! why an even split is worse than none here.

use crate::ssa::branch_prob::cold_arms;
use crate::ssa::dominator::DominatorTree;
use crate::ssa::id_hash::HashMap;
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

    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for b in &func.blocks {
        match &b.term {
            Terminator::Br(t) => preds[t.0 as usize].push(b.id),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                preds[then_blk.0 as usize].push(b.id);
                if else_blk != then_blk {
                    preds[else_blk.0 as usize].push(b.id);
                }
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    let edge_odds: HashMap<(BlockId, BlockId), f64> = cold_arms(func)
        .into_iter()
        .map(|(from, cold, p)| ((from, cold), p))
        .collect();

    // In reverse postorder a block's forward predecessors come first.
    // Its odds are the mass its incoming edges carry, capped by its
    // immediate dominator's; a predecessor not yet visited (a back
    // edge) leaves the cap.
    let mut odds = vec![f64::NAN; n];
    for &b in dom.reverse_postorder() {
        odds[b.0 as usize] = match dom.immediate_dominator(b) {
            None => 1.0,
            Some(d) => {
                let inflow = preds[b.0 as usize]
                    .iter()
                    .filter(|p| dom.is_reachable(**p))
                    .map(|p| odds[p.0 as usize] * edge_odds.get(&(*p, b)).copied().unwrap_or(1.0))
                    .sum::<f64>();
                let cap = odds[d.0 as usize];
                if inflow.is_nan() {
                    cap
                } else {
                    cap.min(inflow)
                }
            }
        };
    }
    for &b in dom.reverse_postorder() {
        let depth = loops.nest_depth(b).min(NEST_CAP);
        let f = (ENTRY_FREQ * LOOP_SCALE.pow(depth)) as f64 * odds[b.0 as usize];
        out[b.0 as usize] = (f.round() as u64).max(1);
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
    fn an_out_of_bounds_read_is_cold() {
        // `xs[i]` reads `undefined` past the end; no call on either arm
        let guard = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 8)),
            inst(2, InstKind::ICmp(IPred::Uge, v(9), v(1))),
        ];
        let f = func(vec![
            block(0, guard, cond_br(2, 1, 2)),
            block(1, vec![], br(3)),
            block(2, vec![], br(3)),
            block(3, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 16, 1024, 1024]);
    }

    /// A header word kept in a register cell across a loop: every def
    /// copies a load of the same word (or the same mask of one), so the
    /// guards on it still predict.
    fn cell_guard(word: Vec<Inst>, cond: InstKind) -> Function {
        let mut insts = word;
        insts.push(inst(3, cond));
        func(vec![
            block(0, insts, cond_br(3, 1, 2)),
            block(1, vec![], br(3)),
            block(2, vec![call()], br(3)),
            block(3, vec![], RET),
        ])
    }

    #[test]
    fn guards_on_a_cached_header_word_still_predict() {
        let len = |at: u32| inst(at, InstKind::Load(Type::I64, v(0), 8));
        let copy = |from: u32| inst(4, InstKind::Copy(Type::I64, v(from)));
        let f = cell_guard(
            vec![len(1), copy(1), len(5), copy(5)],
            InstKind::ICmp(IPred::Ult, v(9), v(4)),
        );
        assert_eq!(block_freqs(&f), vec![1024, 1024, 16, 1024]);
        let flags = |at: u32| inst(at, InstKind::Load(Type::I64, v(0), 0));
        let mask = |at: u32, of: u32| {
            inst(
                at,
                InstKind::BinOp(BinOp::And, v(of), Operand::ConstI64(1 << 49)),
            )
        };
        let f = cell_guard(
            vec![flags(1), mask(2, 1), copy(2), flags(5), mask(6, 5), copy(6)],
            InstKind::ICmp(IPred::Ne, v(4), Operand::ConstI64(0)),
        );
        assert_eq!(block_freqs(&f), vec![1024, 1024, 16, 1024]);
        // a flags cell that also takes some other value predicts nothing
        let f = cell_guard(
            vec![flags(1), mask(2, 1), copy(2), copy(9)],
            InstKind::ICmp(IPred::Ne, v(4), Operand::ConstI64(0)),
        );
        assert_eq!(block_freqs(&f), vec![1024, 1024, 1024, 1024]);
    }

    #[test]
    fn the_empty_arm_of_a_shift_on_a_promoted_length_is_cold() {
        // `len_promote` reads the word into cell %5 ahead of the loop and
        // redefines it on the length store; the guard is `0 >=u len`
        let entry = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 8)),
            inst(5, InstKind::Copy(Type::I64, v(1))),
        ];
        let guard = vec![
            inst(6, InstKind::Copy(Type::I64, v(5))),
            inst(2, InstKind::ICmp(IPred::Uge, Operand::ConstI64(0), v(6))),
        ];
        let store = vec![inst(5, InstKind::Copy(Type::I64, Operand::ConstI64(7)))];
        let f = func(vec![
            block(0, entry, br(1)),
            block(1, guard, cond_br(2, 2, 3)),
            block(2, vec![], br(4)),
            block(3, store, br(4)),
            block(4, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 16, 1024, 1024]);
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

    /// `cond` on `bits` in block 0; block 2 calls, block 1 does not.
    fn guarded_call(bits: Vec<Inst>, cond: InstKind) -> Vec<u64> {
        let mut insts = bits;
        insts.push(inst(3, cond));
        block_freqs(&func(vec![
            block(0, insts, cond_br(3, 1, 2)),
            block(1, vec![], br(3)),
            block(2, vec![call()], br(3)),
            block(3, vec![], RET),
        ]))
    }

    #[test]
    fn an_element_kind_guard_predicts_its_calling_arm_cold() {
        // `(header & KIND_MASK) == KIND`, the kind minted by iconst_hoist
        let bits = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(-1 << 48)),
            ),
            inst(4, InstKind::Copy(Type::I64, Operand::ConstI64(0x5ab << 48))),
        ];
        let cond = InstKind::ICmp(IPred::Eq, v(2), v(4));
        assert_eq!(guarded_call(bits, cond), vec![1024, 1024, 16, 1024]);
        // a field past the header is user data
        let field = vec![
            inst(1, InstKind::Load(Type::I64, v(0), 32)),
            inst(2, InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(4))),
        ];
        let cond = InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0));
        assert_eq!(guarded_call(field, cond), vec![1024, 1024, 1024, 1024]);
    }

    #[test]
    fn a_mask_of_a_stack_slot_predicts_nothing() {
        // `slot = ...; if ((slot & 4) !== 0) f()` with the slot not lifted
        let bits = vec![
            inst(0, InstKind::Alloca(Type::I64)),
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(2, InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(4))),
        ];
        let cond = InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0));
        assert_eq!(guarded_call(bits, cond), vec![1024, 1024, 1024, 1024]);
        // an array cell promoted to the stack keeps a real header word
        let bits = vec![
            inst(0, InstKind::AllocaBytes(168)),
            inst(1, InstKind::Load(Type::I64, v(0), 0)),
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(1 << 49)),
            ),
        ];
        let cond = InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0));
        assert_eq!(guarded_call(bits, cond), vec![1024, 1024, 16, 1024]);
    }

    #[test]
    fn a_tag_test_on_a_boxed_value_predicts_its_calling_arm_cold() {
        let bits = |rest: Vec<Inst>| {
            let mut insts = vec![inst(1, InstKind::PtrToInt(v(0)))];
            insts.extend(rest);
            insts
        };
        // the null test of a loaded element
        let cond = InstKind::ICmp(IPred::Ne, v(1), Operand::ConstI64(0));
        assert_eq!(guarded_call(bits(vec![]), cond), vec![1024, 1024, 16, 1024]);
        // is-int: `lshr bits, 48 == INT_TAG`
        let tag = vec![inst(
            2,
            InstKind::BinOp(BinOp::LShr, v(1), Operand::ConstI64(48)),
        )];
        let cond = InstKind::ICmp(IPred::Eq, v(2), Operand::ConstI64(0xfffe));
        assert_eq!(guarded_call(bits(tag), cond), vec![1024, 1024, 16, 1024]);
        // is-cell: no tag bits and not null
        let cell = vec![
            inst(
                2,
                InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(-0xfffe)),
            ),
            inst(5, InstKind::ICmp(IPred::Eq, v(2), Operand::ConstI64(0))),
            inst(6, InstKind::ICmp(IPred::Ne, v(1), Operand::ConstI64(0))),
        ];
        let cond = InstKind::BinOp(BinOp::And, v(5), v(6));
        assert_eq!(guarded_call(bits(cell), cond), vec![1024, 1024, 16, 1024]);
    }

    #[test]
    fn the_generic_arm_of_an_int_pair_test_is_cold() {
        // `a + b` on two any values: both tags int → the int fast path
        let mut insts = Vec::new();
        for (bits, tag, eq, of) in [(1, 2, 5, 0), (3, 4, 6, 7)] {
            insts.push(inst(bits, InstKind::PtrToInt(v(of))));
            insts.push(inst(
                tag,
                InstKind::BinOp(BinOp::LShr, v(bits), Operand::ConstI64(48)),
            ));
            insts.push(inst(
                eq,
                InstKind::ICmp(IPred::Eq, v(tag), Operand::ConstI64(0xfffe)),
            ));
        }
        insts.push(inst(8, InstKind::BinOp(BinOp::And, v(5), v(6))));
        let pair = |insts: Vec<Inst>| {
            block_freqs(&func(vec![
                block(0, insts, cond_br(8, 1, 2)),
                block(1, vec![], br(3)),
                block(2, vec![], br(3)),
                block(3, vec![], RET),
            ]))
        };
        assert_eq!(pair(insts.clone()), vec![1024, 1024, 16, 1024]);
        // `total + +s`: the right operand is the generic kernel's
        // answer, a double whenever s was a string
        insts.insert(0, inst(7, InstKind::Call(FuncId(0), vec![])));
        assert_eq!(pair(insts), vec![1024, 1024, 1024, 1024]);
    }

    #[test]
    fn a_users_bit_test_predicts_nothing() {
        // `if ((x & 1) === 1) f()` on a number the program computed
        let bits = vec![inst(
            2,
            InstKind::BinOp(BinOp::And, v(0), Operand::ConstI64(1)),
        )];
        let cond = InstKind::ICmp(IPred::Eq, v(2), Operand::ConstI64(1));
        assert_eq!(guarded_call(bits, cond), vec![1024, 1024, 1024, 1024]);
    }

    #[test]
    fn a_slow_call_every_guard_shares_stays_cold() {
        // two kind guards in a row fall back to one kernel call in 3
        let guard = |at: u32| {
            vec![
                inst(at, InstKind::Load(Type::I64, v(0), 0)),
                inst(
                    at + 1,
                    InstKind::BinOp(BinOp::And, v(at), Operand::ConstI64(1 << 49)),
                ),
                inst(
                    at + 2,
                    InstKind::ICmp(IPred::Ne, v(at + 1), Operand::ConstI64(0)),
                ),
            ]
        };
        let f = func(vec![
            block(0, guard(1), cond_br(3, 3, 1)),
            block(1, guard(4), cond_br(6, 3, 2)),
            block(2, vec![], br(4)),
            block(3, vec![call()], br(4)),
            block(4, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 1024, 32, 1024]);
    }

    fn flag_guard(at: u32) -> Vec<Inst> {
        vec![
            inst(at, InstKind::Load(Type::I64, v(0), 0)),
            inst(
                at + 1,
                InstKind::BinOp(BinOp::And, v(at), Operand::ConstI64(1 << 49)),
            ),
            inst(
                at + 2,
                InstKind::ICmp(IPred::Ne, v(at + 1), Operand::ConstI64(0)),
            ),
        ]
    }

    #[test]
    fn a_join_sums_its_edges_and_is_capped_by_its_dominator() {
        // three guards share one slow call: three cold edges add up
        let f = func(vec![
            block(0, flag_guard(1), cond_br(3, 4, 1)),
            block(1, flag_guard(4), cond_br(6, 4, 2)),
            block(2, flag_guard(7), cond_br(9, 4, 3)),
            block(3, vec![], br(5)),
            block(4, vec![call()], br(5)),
            block(5, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 1024, 1024, 48, 1024]);
        // a cold arm, its warm sibling and the other side of an unread
        // split meet: the sum passes 1 and the dominator caps it
        let f = func(vec![
            block(0, vec![], cond_br(100, 1, 2)),
            block(1, flag_guard(1), cond_br(3, 3, 4)),
            block(2, vec![], br(4)),
            block(3, vec![call()], br(4)),
            block(4, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 1024, 1024, 16, 1024]);
    }

    #[test]
    fn a_loop_inside_a_cold_arm_scales_from_the_cold_odds() {
        // the slow arm 1 calls, then loops in 2-3; the loop exit rejoins
        // the warm arm at 4
        let f = func(vec![
            block(0, flag_guard(1), cond_br(3, 1, 4)),
            block(1, vec![call()], br(2)),
            block(2, vec![], cond_br(100, 3, 4)),
            block(3, vec![], br(2)),
            block(4, vec![], RET),
        ]);
        assert_eq!(block_freqs(&f), vec![1024, 16, 256, 256, 1024]);
    }

    #[test]
    fn an_unreachable_block_has_no_frequency() {
        let f = func(vec![block(0, vec![], RET), block(1, vec![], RET)]);
        assert_eq!(block_freqs(&f), vec![1024, 0]);
    }
}
