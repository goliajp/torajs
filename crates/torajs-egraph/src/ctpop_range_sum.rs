//! Ctpop-range-sum reduction recognition — collapses a counted loop
//! whose entire body is `acc += ctpop(counter)` into the single
//! `InstKind::CtpopRangeSum` super-instruction (RFC
//! 20260719-ctpop-range-sum blade 2). The LLVM
//! `LoopIdiomRecognize` approach — replace a recognized idiom with a
//! canned optimal form — applied to a reduction: codegen emits an
//! 8-wide SIMD reduction loop for the whole thing.
//!
//! The shape this fires on is what the pipeline produces for
//! `while (i < K) { total += popcount(i); i = i + 1 }` once
//! `ctpop_idiom` has folded the inner Kernighan loop to one `ctpop`
//! and `branch_fold` has proven the float_demote growth guards
//! false, leaving a straight-line block chain:
//!
//! ```text
//! header:  %c = icmp slt %i, K          ; counted-loop exit test
//!          cond_br %c, body0, exit
//! body0:   %n  = copy i64 %i            ; (chain of Br-only blocks)
//! ...      %pc = ctpop %n
//!          %s  = add %cnt, %pc
//!          %t  = add %total, %s
//!          %i2 = add %i, 1
//!          %total = copy i64 %t
//!          %i     = copy i64 %i2
//!          br header
//! ```
//!
//! and becomes
//!
//! ```text
//! header:  %sum   = ctpop.range.sum %i, K, %total
//!          %total = copy i64 %sum
//!          br exit
//! body*:   unreachable
//! ```
//!
//! Rather than matching each instruction positionally (brittle — the
//! surviving inner-popcount cell dance is an artifact of two earlier
//! passes), the body is *symbolically evaluated* in program order
//! over a four-point domain (`Counter` / `Acc` / constants / `ctpop`
//! and `add` of those). The loop fires only when that evaluation
//! proves the counter cell ends at `Counter + 1` and the accumulator
//! cell at `Acc + ctpop(Counter)`. Extra instructions can't sneak
//! through: every body inst must be `Copy(i64)` / `add` / `ctpop`
//! (so the body is provably free of calls, memory effects, and side
//! exits) and the only value defined in the loop that any block
//! outside reads must be the accumulator itself.
//!
//! Soundness of the collapse: the rewritten header executes once,
//! reading the counter and accumulator cells while they still hold
//! their loop-entry values, so `acc_init + Σ ctpop(i)` over
//! `[start, bound)` is exactly what the deleted loop computed
//! (wrapping add semantics match — the canned emit accumulates in
//! i64 too). Everything else the body defined is unobservable after
//! the loop, which is precisely what the live-out check establishes.
//!
//! Runs after `branch_fold` (the pass that flattens the body to the
//! straight-line chain matched here) and before `block_layout`.
//! `TORAJS_CTPOP_RANGESUM_OFF=1` skips (bisect gate),
//! `TORAJS_CTPOP_RANGESUM_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ast::ExprId;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo,
};

use crate::dominator::DominatorTree;
use crate::loop_analysis::{LoopAnalysis, NaturalLoop};
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CtpopRangeSumStats {
    /// Counted `acc += ctpop(i)` loops replaced by the super-inst.
    pub loops_rewritten: u32,
}

/// Run reduction recognition over every function body.
pub fn form_ctpop_range_sums(module: &mut Module) -> CtpopRangeSumStats {
    let mut stats = CtpopRangeSumStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let dom = DominatorTree::compute(func);
        let la = LoopAnalysis::compute(func, &dom);
        let matches: Vec<Matched> = la
            .loops()
            .iter()
            .filter_map(|lp| match_loop(func, lp))
            .collect();
        for m in &matches {
            rewrite_loop(func, m);
            stats.loops_rewritten += 1;
        }
    }
    stats
}

/// Symbolic value of an SSA value at a point in the loop body, in
/// terms of the two loop-carried cells' iteration-entry values.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Sym {
    /// The counter cell as it entered this iteration.
    Counter,
    /// The accumulator cell as it entered this iteration.
    Acc,
    Const(i64),
    Ctpop(Box<Sym>),
    Add(Box<Sym>, Box<Sym>),
    /// Anything the domain can't track (loop-invariant values, cells
    /// read before their in-iteration def, unsupported operands).
    Unknown,
}

/// Constant-folding `add` constructor. Folding the `+ 0` identity is
/// load-bearing, not cosmetic: the inner popcount's count cell is
/// seeded `copy 0` each iteration, so the accumulated term arrives as
/// `Add(Const(0), Ctpop(Counter))` and would otherwise miss the
/// shape check.
fn sym_add(a: Sym, b: Sym) -> Sym {
    match (a, b) {
        (Sym::Const(x), Sym::Const(y)) => match x.checked_add(y) {
            Some(s) => Sym::Const(s),
            None => Sym::Unknown,
        },
        (Sym::Const(0), o) | (o, Sym::Const(0)) => o,
        (x, y) => Sym::Add(Box::new(x), Box::new(y)),
    }
}

/// Is `s` an `add` of the two wanted terms, in either operand order?
fn is_add_of(s: &Sym, want_a: &Sym, want_b: &Sym) -> bool {
    match s {
        Sym::Add(x, y) => (**x == *want_a && **y == *want_b) || (**x == *want_b && **y == *want_a),
        _ => false,
    }
}

struct Matched {
    header: BlockId,
    body_blocks: Vec<BlockId>,
    exit: BlockId,
    counter: ValueId,
    bound: Operand,
    acc: ValueId,
    origin: Option<ExprId>,
}

fn match_loop(func: &Function, lp: &NaturalLoop) -> Option<Matched> {
    let header_blk = block(func, lp.header)?;
    // header: exactly `%c = icmp slt %counter, K` + `cond_br %c`.
    // `slt` with a unit step is the only pred the range semantics
    // map onto directly; anything else is a safe non-match.
    if header_blk.insts.len() != 1 {
        return None;
    }
    let cmp_inst = &header_blk.insts[0];
    let cmp = cmp_inst.result?;
    let InstKind::ICmp(IPred::Slt, Operand::Value(counter), bound) = &cmp_inst.kind else {
        return None;
    };
    let (counter, bound) = (*counter, *bound);
    let Terminator::CondBr {
        cond: Operand::Value(cv),
        then_blk,
        else_blk,
    } = &header_blk.term
    else {
        return None;
    };
    if *cv != cmp || !lp.body.contains(then_blk) || lp.body.contains(else_blk) {
        return None;
    }
    if !operand_invariant(func, lp, &bound) {
        return None;
    }

    // The body must be a single straight-line chain of Br-only
    // blocks running from the loop entry back to the header — no
    // side exits, no inner loop, and a well-defined program order
    // for the symbolic evaluation below.
    let body_blocks = body_chain(func, lp, *then_blk)?;

    // Exactly one value defined inside the loop may be read outside
    // it: the accumulator. Everything else the body computes has to
    // be unobservable once the loop is gone.
    let live_out = live_out_defs(func, lp);
    let [acc] = live_out[..] else {
        return None;
    };
    if acc == counter {
        return None;
    }

    let env = eval_body(func, &body_blocks, counter, acc)?;
    if !is_add_of(env.get(&counter)?, &Sym::Counter, &Sym::Const(1)) {
        return None;
    }
    let want = Sym::Ctpop(Box::new(Sym::Counter));
    if !is_add_of(env.get(&acc)?, &Sym::Acc, &want) {
        return None;
    }

    Some(Matched {
        header: lp.header,
        body_blocks,
        exit: *else_blk,
        counter,
        bound,
        acc,
        origin: cmp_inst.origin,
    })
}

fn block(func: &Function, id: BlockId) -> Option<&torajs_core::ssa::Block> {
    func.blocks.iter().find(|b| b.id == id)
}

/// Walk `Br`-only successors from `start` until the header, checking
/// every block is a fresh member of the loop body. Returns the chain
/// (loop body minus the header) or `None` for any branching shape.
fn body_chain(func: &Function, lp: &NaturalLoop, start: BlockId) -> Option<Vec<BlockId>> {
    let mut chain: Vec<BlockId> = Vec::new();
    let mut seen: HashSet<BlockId> = HashSet::new();
    let mut cur = start;
    loop {
        if !lp.body.contains(&cur) || !seen.insert(cur) {
            return None;
        }
        chain.push(cur);
        let Terminator::Br(next) = block(func, cur)?.term else {
            return None;
        };
        if next == lp.header {
            break;
        }
        cur = next;
    }
    // Every body block must be on the chain — a block reachable only
    // through some other edge would execute outside this order.
    if chain.len() + 1 != lp.body.len() {
        return None;
    }
    Some(chain)
}

/// Values defined anywhere in the loop that some block outside reads.
/// Multi-def cells count: a reader outside may observe either def, so
/// including them here only tightens the match.
fn live_out_defs(func: &Function, lp: &NaturalLoop) -> Vec<ValueId> {
    let mut defined: HashSet<ValueId> = HashSet::new();
    for b in func.blocks.iter().filter(|b| lp.body.contains(&b.id)) {
        for inst in &b.insts {
            if let Some(r) = inst.result {
                defined.insert(r);
            }
        }
    }
    let mut out: HashSet<ValueId> = HashSet::new();
    for b in func.blocks.iter().filter(|b| !lp.body.contains(&b.id)) {
        let mut check = |v: ValueId| {
            if defined.contains(&v) {
                out.insert(v);
            }
        };
        for inst in &b.insts {
            visit_value_operands(&inst.kind, &mut check);
        }
        match &b.term {
            Terminator::Ret(Some(Operand::Value(v))) => check(*v),
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => check(*v),
            _ => {}
        }
    }
    let mut v: Vec<ValueId> = out.into_iter().collect();
    v.sort_by_key(|x| x.0);
    v
}

/// Is `op` defined outside the loop (or a constant)?
fn operand_invariant(func: &Function, lp: &NaturalLoop, op: &Operand) -> bool {
    match op {
        Operand::ConstI64(_) => true,
        Operand::Value(v) => !func
            .blocks
            .iter()
            .filter(|b| lp.body.contains(&b.id))
            .any(|b| b.insts.iter().any(|i| i.result == Some(*v))),
        _ => false,
    }
}

/// Symbolically execute the body chain in program order. Returns the
/// final environment, or `None` if any instruction falls outside the
/// tracked opcode set (which is also what proves the body has no
/// calls, memory effects, or other observable behaviour).
fn eval_body(
    func: &Function,
    chain: &[BlockId],
    counter: ValueId,
    acc: ValueId,
) -> Option<HashMap<ValueId, Sym>> {
    let mut env: HashMap<ValueId, Sym> = HashMap::new();
    env.insert(counter, Sym::Counter);
    env.insert(acc, Sym::Acc);
    for bid in chain {
        for inst in &block(func, *bid)?.insts {
            let r = inst.result?;
            let s = match &inst.kind {
                InstKind::Copy(Type::I64, op) => eval_op(&env, op),
                InstKind::BinOp(BinOp::Add, a, b) => sym_add(eval_op(&env, a), eval_op(&env, b)),
                InstKind::Ctpop(op) => Sym::Ctpop(Box::new(eval_op(&env, op))),
                _ => return None,
            };
            env.insert(r, s);
        }
    }
    Some(env)
}

fn eval_op(env: &HashMap<ValueId, Sym>, op: &Operand) -> Sym {
    match op {
        Operand::Value(v) => env.get(v).cloned().unwrap_or(Sym::Unknown),
        Operand::ConstI64(c) => Sym::Const(*c),
        _ => Sym::Unknown,
    }
}

fn rewrite_loop(func: &mut Function, m: &Matched) {
    let sum = ValueId(func.values.len() as u32);
    func.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    let mut insts = vec![
        Inst {
            result: Some(sum),
            kind: InstKind::CtpopRangeSum(
                Operand::Value(m.counter),
                m.bound,
                Operand::Value(m.acc),
            ),
            origin: m.origin,
        },
        Inst {
            result: Some(m.acc),
            kind: InstKind::Copy(Type::I64, Operand::Value(sum)),
            origin: m.origin,
        },
    ];
    for block in func.blocks.iter_mut() {
        if block.id == m.header {
            block.insts = std::mem::take(&mut insts);
            block.term = Terminator::Br(m.exit);
        } else if m.body_blocks.contains(&block.id) {
            block.insts.clear();
            block.term = Terminator::Unreachable;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Function, Module};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

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

    /// The post-branch_fold popcount shape, structurally faithful to
    /// the `TORAJS_SSA_DUMP` capture (values renumbered):
    ///   bb0 entry: total=0, i=0
    ///   bb1 header: %10 = icmp slt i, 1e7 → bb2 / bb4
    ///   bb2: n=i, cnt=0, %11=ctpop n, %12=add cnt,%11, cnt=%12, n=0
    ///   bb3: %13=copy cnt, %14=add total,%13, %15=add i,1,
    ///        total=%14, i=%15 → bb1
    ///   bb4 exit: ret total
    ///
    /// Cells: total=%0, i=%1, n=%2, cnt=%3.
    fn popcount_outer_module(body_extra: Vec<Inst>, exit_term: Terminator) -> Module {
        let mut values: Vec<ValueInfo> = (0..16)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect();
        values[10].ty = Type::Bool;
        let mut bb2 = vec![
            inst(2, InstKind::Copy(Type::I64, v(1))),
            inst(3, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            inst(11, InstKind::Ctpop(v(2))),
            inst(12, InstKind::BinOp(BinOp::Add, v(3), v(11))),
            inst(3, InstKind::Copy(Type::I64, v(12))),
            inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
        ];
        bb2.extend(body_extra);
        let func = Function {
            name: "outer".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![
                blk(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                        inst(1, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                blk(
                    1,
                    vec![inst(
                        10,
                        InstKind::ICmp(IPred::Slt, v(1), Operand::ConstI64(10_000_000)),
                    )],
                    Terminator::CondBr {
                        cond: v(10),
                        then_blk: BlockId(2),
                        else_blk: BlockId(4),
                    },
                ),
                blk(2, bb2, Terminator::Br(BlockId(3))),
                blk(
                    3,
                    vec![
                        inst(13, InstKind::Copy(Type::I64, v(3))),
                        inst(14, InstKind::BinOp(BinOp::Add, v(0), v(13))),
                        inst(15, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
                        inst(0, InstKind::Copy(Type::I64, v(14))),
                        inst(1, InstKind::Copy(Type::I64, v(15))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                blk(4, Vec::new(), exit_term),
            ],
            current_origin: None,
        };
        Module {
            funcs: vec![func],
            ..Default::default()
        }
    }

    fn popcount_outer() -> Module {
        popcount_outer_module(Vec::new(), Terminator::Ret(Some(v(0))))
    }

    #[test]
    fn counted_ctpop_reduction_collapses_to_super_inst() {
        let mut m = popcount_outer();
        let stats = form_ctpop_range_sums(&mut m);
        assert_eq!(stats.loops_rewritten, 1);

        let f = &m.funcs[0];
        let header = f.blocks.iter().find(|b| b.id == BlockId(1)).unwrap();
        assert_eq!(header.insts.len(), 2);
        // start = counter cell, bound = the header compare's K,
        // acc_init = accumulator cell — all read at their loop-entry
        // values now that the header runs once.
        assert!(matches!(
            &header.insts[0].kind,
            InstKind::CtpopRangeSum(
                Operand::Value(ValueId(1)),
                Operand::ConstI64(10_000_000),
                Operand::Value(ValueId(0)),
            )
        ));
        assert_eq!(header.insts[1].result, Some(ValueId(0)));
        assert!(matches!(header.term, Terminator::Br(BlockId(4))));
        for b in f.blocks.iter().filter(|b| matches!(b.id.0, 2 | 3)) {
            assert!(b.insts.is_empty());
            assert!(matches!(b.term, Terminator::Unreachable));
        }
    }

    /// A call in the body is outside the tracked opcode set — the
    /// straight-line purity argument fails, so no fire.
    #[test]
    fn call_in_body_blocks_fire() {
        let mut m = popcount_outer_module(
            vec![inst(
                9,
                InstKind::Call(torajs_core::ssa::FuncId(0), Vec::new()),
            )],
            Terminator::Ret(Some(v(0))),
        );
        assert_eq!(form_ctpop_range_sums(&mut m).loops_rewritten, 0);
    }

    /// A second live-out (here the counter, read after the loop)
    /// means deleting the body would strand a reader.
    #[test]
    fn extra_live_out_blocks_fire() {
        let mut m = popcount_outer_module(
            Vec::new(),
            Terminator::Ret(Some(v(1))), // exit reads the counter
        );
        assert_eq!(form_ctpop_range_sums(&mut m).loops_rewritten, 0);
    }

    /// Stepping the counter by 2 breaks the `[start, bound)` unit
    /// range the super-inst sums over.
    #[test]
    fn non_unit_counter_step_blocks_fire() {
        let mut m = popcount_outer();
        let f = &mut m.funcs[0];
        let latch = f.blocks.iter_mut().find(|b| b.id == BlockId(3)).unwrap();
        latch.insts[2].kind = InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(2));
        assert_eq!(form_ctpop_range_sums(&mut m).loops_rewritten, 0);
    }

    /// Accumulating `ctpop` of something other than the counter is a
    /// different reduction — the canned emit only knows the IV one.
    #[test]
    fn ctpop_of_non_counter_blocks_fire() {
        let mut m = popcount_outer();
        let f = &mut m.funcs[0];
        let body = f.blocks.iter_mut().find(|b| b.id == BlockId(2)).unwrap();
        // seed n from the accumulator instead of the counter
        body.insts[0].kind = InstKind::Copy(Type::I64, v(0));
        assert_eq!(form_ctpop_range_sums(&mut m).loops_rewritten, 0);
    }

    /// A side exit inside the body defeats the straight-line chain
    /// (and with it the program order the evaluation relies on).
    #[test]
    fn side_exit_in_body_blocks_fire() {
        let mut m = popcount_outer();
        let f = &mut m.funcs[0];
        let body = f.blocks.iter_mut().find(|b| b.id == BlockId(2)).unwrap();
        body.term = Terminator::CondBr {
            cond: v(10),
            then_blk: BlockId(3),
            else_blk: BlockId(4),
        };
        assert_eq!(form_ctpop_range_sums(&mut m).loops_rewritten, 0);
    }
}
