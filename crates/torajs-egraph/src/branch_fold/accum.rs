//! Loop-accumulator bound — the SCEV-lite half of branch_fold (LLVM
//! ScalarEvolution add-recurrence analogue). A guarded value `%x =
//! add %acc, %inc` whose cell `%acc` is a plain add-recurrence over a
//! counted loop is bounded by `entry + trip * inc` even though the
//! interval lattice widens the cell to a sentinel: the float_demote
//! accumulator guard (`total > 2^53-1` on a popcount sum) is exactly
//! this shape, with `trip = 1e7` from the loop-counter compare and
//! `inc ∈ [0, 64]` from the ctpop fact.
//!
//! Soundness conditions (all checked structurally):
//! * the recurrence def (`%acc = copy %x`, or `%acc = copy %m` of a
//!   cell `%m` every path through the iteration sets to `%x` or
//!   `%acc` — a conditional `if (c) s += d`) is the only def of `%acc`
//!   inside the loop, and it and `%x` sit at the loop's own nest depth
//!   (no inner loop re-executes them within one header visit — a body
//!   cycle avoiding the header would be its own natural loop and
//!   deepen the nest). The bound holds per episode of the loop: a
//!   nested loop starts each one from what `PointEval` says of the
//!   entry, at every entry;
//! * the loop has a single back edge and its header exits on
//!   `counter <slt/sle> K` with the in-loop side on `then`; `K` is a
//!   constant or a value with a fact at the compare (a tracked array's
//!   length read after its push loop's writeback) — a fact holds at
//!   every header visit, so its high end bounds the trips whatever `K`
//!   does within it. Without such a header, a counter whose every
//!   value the lattice bounds counts the trips instead (the trial
//!   divisor under `d * d <= n`);
//! * the counter's only in-loop def is `copy (add counter, s)` with
//!   constant step `s >= 1`, in a block that dominates the latch;
//! * entry values for both cells come from `PointEval` at every
//!   header predecessor outside the loop (join over entries).
//!
//! An f64 sum (`fadd`) of integer-valued steps is bounded the same
//! way when the whole bound lies inside ±2^53: every partial sum is
//! then exact.
//!
//! The increment is evaluated at the add's point; when that read is
//! the loop counter itself (`total += i` — through the block-tail
//! `copy` the post-φ form threads it through), point evaluation has
//! no unique last def inside the body (the latch def and the entry
//! def both reach) and answers nothing. The counter's own trip
//! argument bounds it instead: at any body point it lies in
//! `[entry, K (−1 for slt) + s]` — the header check bounds it at
//! iteration start and one step is all a body pass adds (r506; the
//! `sum += i` guard never folded before, so its slow version and
//! f64 exit bridge stayed). An increment read out of an array
//! (`sum += xs[i]`) has no def at any point; the lattice's element
//! point is met in for it.

use torajs_core::ssa::{BinOp, BlockId, Function, InstKind, Operand, ValueId};

use super::counter::{entry_fact, loop_counter, resolves_to};
use super::point_eval::PointEval;
use crate::interval::transfer::NumFact;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};

const F64_EXACT: i128 = 1 << 53;

/// Bound `x` (a single-def `add cell, inc` result) via the
/// add-recurrence argument. Returns a fact valid at every execution
/// of `x`'s def.
pub(crate) fn accum_bound(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    x: ValueId,
    x_def: (usize, usize),
) -> Option<NumFact> {
    let inst = &func.blocks[x_def.0].insts[x_def.1];
    let (float, a, b) = match &inst.kind {
        InstKind::BinOp(BinOp::Add, a, b) => (false, a, b),
        InstKind::BinOp(BinOp::FAdd, a, b) => (true, a, b),
        _ => return None,
    };
    // One side is the accumulator cell, the other the increment.
    let (acc, inc_op) = match (a, b) {
        (Operand::Value(va), _) if is_rec_cell(func, *va, x) => (*va, b),
        (_, Operand::Value(vb)) if is_rec_cell(func, *vb, x) => (*vb, a),
        _ => return None,
    };
    let x_block = func.blocks[x_def.0].id;
    let lp = innermost_loop(la, x_block)?;
    let depth = la.nest_depth(lp.header);
    if lp.back_edge_sources.len() != 1 || la.nest_depth(x_block) != depth {
        return None;
    }
    let rec_def = sole_rec_def(func, pe, la, lp, acc, x)?;
    if la.nest_depth(func.blocks[rec_def.0].id) != depth {
        return None;
    }
    let counter = loop_counter(func, lp, pe)?;
    // an add in the header runs once more than the body: on the visit
    // that leaves too
    let trip = counter.trip() + i128::from(x_block == lp.header);
    let entry = entry_fact(func, lp, pe, acc)?;
    let inc = match pe.eval_operand_with_flow(inc_op, x_def) {
        Some(f) => f,
        None if resolves_to(func, pe, inc_op, counter.cell) => counter.body_range(),
        None => return None,
    };
    // After k <= trip additions the checked (post-add) value is
    // entry + k * inc; bound each side by the worst-signed growth.
    let hi = entry.hi.checked_add(trip.checked_mul(inc.hi.max(0))?)?;
    let lo = entry.lo.checked_add(trip.checked_mul(inc.lo.min(0))?)?;
    // f64 sums of integers are exact, step by step, only while every
    // partial sum stays inside ±2^53
    if float && (lo <= -F64_EXACT || hi >= F64_EXACT) {
        return None;
    }
    Some(NumFact::new(lo, hi))
}

/// Bound a loop cell read AFTER its loop (the `console.log(sum)` /
/// `return sum` accumulator shape — the exit bridge's `sitofp` reads
/// it there): the join of the cell's entry value and the recurrence
/// bound of its in-loop def. Sound when every def of the cell is
/// either the in-loop recurrence def or sits in a header predecessor
/// outside the loop, and the read point is dominated by the header
/// (so every path to it entered the loop through the same entries).
pub(crate) fn cell_bound_after_loop(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    dom: &torajs_core::ssa::dominator::DominatorTree,
    cell: ValueId,
    point: (usize, usize),
) -> Option<NumFact> {
    // the one in-loop def `copy x`, and every other def on an entry edge
    let mut rec: Option<(ValueId, BlockId)> = None;
    let mut lp: Option<&NaturalLoop> = None;
    for block in &func.blocks {
        for inst in &block.insts {
            if inst.result != Some(cell) {
                continue;
            }
            if let Some(l) = innermost_loop(la, block.id) {
                if rec.is_some() {
                    return None;
                }
                let InstKind::Copy(_, Operand::Value(x)) = &inst.kind else {
                    return None;
                };
                rec = Some((*x, block.id));
                lp = Some(l);
            }
        }
    }
    let (x, _) = rec?;
    let lp = lp?;
    let point_block = func.blocks[point.0].id;
    if lp.contains(&point_block) || !dom.dominates(lp.header, point_block) {
        return None;
    }
    for block in &func.blocks {
        if lp.contains(&block.id) {
            continue;
        }
        let defines = block.insts.iter().any(|i| i.result == Some(cell));
        if defines && !super::point_eval::successors(&block.term).any(|s| s == lp.header) {
            return None;
        }
    }
    let x_def = pe.single_def(x)?;
    let grown = accum_bound(func, la, pe, x, x_def)?;
    let entry = entry_fact(func, lp, pe, cell)?;
    Some(entry.join(grown))
}

/// The recurrence def of the accumulator `acc` stepped by `x`: it must
/// be the only def of the cell in the loop, copying `x` or a merge of
/// the step.
fn sole_rec_def(
    func: &Function,
    pe: &PointEval,
    la: &LoopAnalysis,
    lp: &NaturalLoop,
    acc: ValueId,
    x: ValueId,
) -> Option<(usize, usize)> {
    let mut rec_def: Option<(usize, usize)> = None;
    for &(bi, ii) in pe.defs.get(&acc).into_iter().flatten() {
        let block = &func.blocks[bi];
        if !lp.contains(&block.id) {
            continue;
        }
        if rec_def.is_some() {
            return None;
        }
        let InstKind::Copy(_, Operand::Value(src)) = &block.insts[ii].kind else {
            return None;
        };
        if *src != x && !merges_step(func, la, lp, acc, x, *src, (bi, ii)) {
            return None;
        }
        rec_def = Some((bi, ii));
    }
    rec_def
}

/// Is `cell` a multi-def value with a `copy x` def somewhere, or a
/// `copy m` of a cell `m` that has one (the cheap pre-filter before
/// the full structural walk)?
fn is_rec_cell(func: &Function, cell: ValueId, x: ValueId) -> bool {
    let copies_of = |dst: ValueId| {
        func.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(move |i| i.result == Some(dst))
            .map(|i| match &i.kind {
                InstKind::Copy(_, Operand::Value(s)) => Some(*s),
                _ => None,
            })
    };
    let defs = copies_of(cell).count();
    let saw_rec = copies_of(cell)
        .flatten()
        .any(|s| s == x || (s != cell && copies_of(s).flatten().any(|t| t == x)));
    defs >= 2 && saw_rec
}

/// The accumulator's in-loop def `acc = copy m` takes this iteration's
/// step or nothing: every def of the cell `m` sits in the loop outside
/// any loop nested in it and is `copy x` or `copy acc`, and every path
/// from the header to the def at `at` passes one of them (else `m`
/// could hand over a value from an earlier episode).
fn merges_step(
    func: &Function,
    la: &LoopAnalysis,
    lp: &NaturalLoop,
    acc: ValueId,
    x: ValueId,
    m: ValueId,
    at: (usize, usize),
) -> bool {
    let depth = la.nest_depth(lp.header);
    let mut def_blocks: Vec<usize> = Vec::new();
    let mut saw_x = false;
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if inst.result != Some(m) {
                continue;
            }
            let src = match &inst.kind {
                InstKind::Copy(_, Operand::Value(s)) if *s == x || *s == acc => *s,
                _ => return false,
            };
            if !lp.contains(&block.id) || la.nest_depth(block.id) != depth {
                return false;
            }
            // a def after the read in its own block covers nothing
            if bi == at.0 && ii > at.1 {
                return false;
            }
            saw_x |= src == x;
            def_blocks.push(bi);
        }
    }
    saw_x && !reaches_avoiding(func, lp, at.0, &def_blocks)
}

/// Whether block `to` is reachable from the header of `lp` inside its
/// body without entering any of `avoid` (back edges not followed).
fn reaches_avoiding(func: &Function, lp: &NaturalLoop, to: usize, avoid: &[usize]) -> bool {
    let start = lp.header.0 as usize;
    if avoid.contains(&start) {
        return false;
    }
    let mut seen = vec![false; func.blocks.len()];
    let mut work = vec![start];
    while let Some(b) = work.pop() {
        if b == to {
            return true;
        }
        if seen[b] {
            continue;
        }
        seen[b] = true;
        for s in super::point_eval::successors(&func.blocks[b].term) {
            let si = s.0 as usize;
            if lp.contains(&s) && s != lp.header && !avoid.contains(&si) {
                work.push(si);
            }
        }
    }
    false
}

fn innermost_loop(la: &LoopAnalysis, b: BlockId) -> Option<&NaturalLoop> {
    la.loops()
        .iter()
        .filter(|l| l.contains(&b))
        .min_by_key(|l| l.body.len())
}

#[cfg(test)]
mod tests {
    use super::super::{BranchFoldStats, fold_in_function};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

    const ALLOC: u32 = 0;
    const RESERVE: u32 = 1;
    const PUSH_UNCHECKED: u32 = 2;

    fn names() -> Vec<String> {
        vec![
            "__torajs_arr_alloc".into(),
            "__torajs_arr_reserve".into(),
            "__torajs_arr_push_unchecked".into(),
        ]
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
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

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond(cond: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(cond),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    /// The parameter-array reduction after inlining: a pre-reserved
    /// push loop writes `i ∈ [0, 1e7)` into a fresh array and stores
    /// the count back as its length, then `for (j = 0; j < xs.length;
    /// j++) sum += xs[j]` runs with float_demote's `sum > 2^53-1` guard.
    /// `xs.length` is the length cell read after the push loop's
    /// writeback and `xs[j]` the element point; both bound the sum.
    ///
    ///   b0: arr=alloc; reserve; n=len; i=0     b4: K=len; j < K ? b5 : b7
    ///   b1: i < 1e7 ? b2 : b3                  b5: e=xs[j]; x=sum+e; x > 2^53-1 ? b8 : b6
    ///   b2: xs[n]=i; n++; i++ → b1             b6: j++; sum=x → b4
    ///   b3: len=n; j=0; sum=0 → b4             b7: ret   b8: side exit
    fn reduction(body_extra: Vec<Inst>) -> Function {
        // %0 arr, %1 len0, %2 cap, %3 arr2, %4 data, %5 len1, %6 i cell,
        // %7 n cell, %8 hdr1, %9 n+1, %10 i+1, %11 K, %12 hdr2, %13 j cell,
        // %14 sum cell, %15 e, %16 x, %17 guard, %18 j+1
        let mut b5 = vec![
            inst(15, InstKind::LoadDyn(Type::I64, v(4), v(13))),
            inst(16, InstKind::BinOp(BinOp::Add, v(14), v(15))),
            inst(17, InstKind::ICmp(IPred::Sgt, v(16), c(9007199254740991))),
        ];
        b5.splice(1..1, body_extra);
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Call(FuncId(ALLOC), vec![c(0)])),
                        inst(1, InstKind::Load(Type::I64, v(0), 8)),
                        inst(2, InstKind::BinOp(BinOp::Add, v(1), c(10_000_000))),
                        inst(3, InstKind::Call(FuncId(RESERVE), vec![v(0), v(2)])),
                        inst(4, InstKind::Load(Type::Ptr, v(3), 32)),
                        inst(5, InstKind::Load(Type::I64, v(3), 8)),
                        inst(6, InstKind::Copy(Type::I64, c(0))),
                        inst(7, InstKind::Copy(Type::I64, v(5))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(8, InstKind::ICmp(IPred::Slt, v(6), c(10_000_000)))],
                    cond(8, 2, 3),
                ),
                block(
                    2,
                    vec![
                        void(InstKind::StoreDyn(v(6), v(4), v(7))),
                        inst(9, InstKind::BinOp(BinOp::Add, v(7), c(1))),
                        inst(10, InstKind::BinOp(BinOp::Add, v(6), c(1))),
                        inst(6, InstKind::Copy(Type::I64, v(10))),
                        inst(7, InstKind::Copy(Type::I64, v(9))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    3,
                    vec![
                        void(InstKind::Store(v(7), v(3), 8)),
                        inst(13, InstKind::Copy(Type::I64, c(0))),
                        inst(14, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(4)),
                ),
                block(
                    4,
                    vec![
                        inst(11, InstKind::Load(Type::I64, v(0), 8)),
                        inst(12, InstKind::ICmp(IPred::Slt, v(13), v(11))),
                    ],
                    cond(12, 5, 7),
                ),
                block(5, b5, cond(17, 8, 6)),
                block(
                    6,
                    vec![
                        inst(18, InstKind::BinOp(BinOp::Add, v(13), c(1))),
                        inst(14, InstKind::Copy(Type::I64, v(16))),
                        inst(13, InstKind::Copy(Type::I64, v(18))),
                    ],
                    Terminator::Br(BlockId(4)),
                ),
                block(7, vec![], Terminator::Ret(None)),
                block(8, vec![], Terminator::Ret(None)),
            ],
            values: (0..19)
                .map(|i| ValueInfo {
                    ty: if matches!(i, 0 | 3 | 4) {
                        Type::Ptr
                    } else {
                        Type::I64
                    },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    #[test]
    fn length_bounded_reduction_guard_folds() {
        let mut f = reduction(vec![]);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &names(), None, &mut stats);
        assert_eq!(stats.folded, 1, "the accumulator guard folds: {stats:?}");
        assert!(
            matches!(f.blocks[5].term, Terminator::Br(BlockId(6))),
            "the guard takes the in-range arm: {:?}",
            f.blocks[5].term
        );
    }

    /// A push inside the reduction loop moves the length by an amount
    /// nothing models, so the header's length read has no unique last
    /// def and the guard stays.
    #[test]
    fn push_in_the_loop_keeps_the_guard() {
        let mut f = reduction(vec![void(InstKind::Call(
            FuncId(PUSH_UNCHECKED),
            vec![v(3), v(15)],
        ))]);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &names(), None, &mut stats);
        assert_eq!(stats.folded, 0, "{stats:?}");
    }

    /// Without callee names no array is tracked: the length read and
    /// the element read are opaque and the guard stays.
    #[test]
    fn untracked_array_keeps_the_guard() {
        let mut f = reduction(vec![]);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 0, "{stats:?}");
    }

    /// `for (i = 0; i < 10; i++) { s = 0; for (d = 1; d * d <= n; d++)
    /// if (n % d === 0) s += d }` with float_demote's `s + d < 2^53`
    /// window check on the add, `n` = `%0`:
    ///
    ///   b0: n; i=0                 b5: x=s+d; x>>53 != 0 ? b10 : b11
    ///   b1: i < 10 ? b2 : b9       b6: m=s → b7       b11: m=x → b7
    ///   b2: s=0; d=1               b7: d++; s=m → b3
    ///   b3: d*d <= n ? b4 : b8     b8: i++ → b1
    ///   b4: n%d == 0 ? b5 : b6     b9, b10: ret
    fn divisor_sums(n: i64, else_merges: bool) -> Function {
        let mut b6 = vec![];
        if else_merges {
            b6.push(inst(12, InstKind::Copy(Type::I64, v(3))));
        }
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, c(n))),
                        inst(1, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(2, InstKind::ICmp(IPred::Slt, v(1), c(10)))],
                    cond(2, 2, 9),
                ),
                block(
                    2,
                    vec![
                        inst(3, InstKind::Copy(Type::I64, c(0))),
                        inst(4, InstKind::Copy(Type::I64, c(1))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![
                        inst(5, InstKind::BinOp(BinOp::Mul, v(4), v(4))),
                        inst(6, InstKind::ICmp(IPred::Sle, v(5), v(0))),
                    ],
                    cond(6, 4, 8),
                ),
                block(
                    4,
                    vec![
                        inst(7, InstKind::BinOp(BinOp::SRem, v(0), v(4))),
                        inst(8, InstKind::ICmp(IPred::Eq, v(7), c(0))),
                    ],
                    cond(8, 5, 6),
                ),
                block(
                    5,
                    vec![
                        inst(9, InstKind::BinOp(BinOp::Add, v(3), v(4))),
                        inst(10, InstKind::BinOp(BinOp::LShr, v(9), c(53))),
                        inst(11, InstKind::ICmp(IPred::Ne, v(10), c(0))),
                    ],
                    cond(11, 10, 11),
                ),
                block(6, b6, Terminator::Br(BlockId(7))),
                block(
                    7,
                    vec![
                        inst(13, InstKind::BinOp(BinOp::Add, v(4), c(1))),
                        inst(3, InstKind::Copy(Type::I64, v(12))),
                        inst(4, InstKind::Copy(Type::I64, v(13))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    8,
                    vec![
                        inst(14, InstKind::BinOp(BinOp::Add, v(1), c(1))),
                        inst(1, InstKind::Copy(Type::I64, v(14))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(9, vec![], Terminator::Ret(None)),
                block(10, vec![], Terminator::Ret(None)),
                block(
                    11,
                    vec![inst(12, InstKind::Copy(Type::I64, v(9)))],
                    Terminator::Br(BlockId(7)),
                ),
            ],
            values: (0..15)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    #[test]
    fn nested_conditional_sum_under_a_square_test_folds() {
        let mut f = divisor_sums(1_000_000, true);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 1, "{stats:?}");
        // the side exit is swept and the blocks after it renumbered:
        // the guard now branches straight to the merge `m = x`
        let Terminator::Br(t) = f.blocks[5].term else {
            panic!("{:?}", f.blocks[5].term);
        };
        let merge = &f.blocks[t.0 as usize].insts;
        assert_eq!(merge[0].kind, InstKind::Copy(Type::I64, v(9)));
    }

    /// The merge cell skipped on one path could hand an earlier
    /// episode's sum to this one.
    #[test]
    fn a_merge_cell_one_path_skips_keeps_the_guard() {
        let mut f = divisor_sums(1_000_000, false);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 0, "{stats:?}");
    }

    /// A divisor whose square may wrap is not bounded, so neither is
    /// the trip count.
    #[test]
    fn an_unbounded_divisor_keeps_the_guard() {
        let mut f = divisor_sums(i64::MAX - 1, true);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 0, "{stats:?}");
    }
}
