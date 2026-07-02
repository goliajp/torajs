//! Kernighan popcount loop-idiom recognition — the LLVM
//! `LoopIdiomRecognize::recognizePopcount` analogue over the post-φ
//! canonical SSA. Matches the exact 2-block natural loop that
//! `while (n !== 0) { n = n & (n - 1); count = count + 1; }` lowers
//! to once `sext_elide` (seeded by the interval analysis) has washed
//! the ToInt32 pairs off the hot path:
//!
//! ```text
//! header:  %cmp = icmp ne %n, 0
//!          cond_br %cmp, body, exit
//! body:    %t  = sub %n, 1
//!          %m  = and %n, %t
//!          %c2 = add %c, 1
//!          %n  = copy i64 %m
//!          %c  = copy i64 %c2
//!          br header
//! ```
//!
//! and replaces the whole loop with straight-line code in the header:
//!
//! ```text
//! header:  %pc  = ctpop %n
//!          %sum = add %c, %pc
//!          %c   = copy i64 %sum
//!          br exit
//! body:    unreachable
//! ```
//!
//! Soundness (all i64 inputs, including negative and i64::MIN): each
//! iteration `n & (n - 1)` clears exactly the lowest set bit of the
//! two's-complement pattern (wrapping `sub` included — for
//! n = i64::MIN, n-1 wraps to i64::MAX and the AND yields 0), so the
//! loop runs exactly `popcount(n)` times and the count cell receives
//! `c + popcount(n)` under the same wrapping-add semantics. Shapes
//! where a surviving `shl 32`+`ashr 32` pair truncates the AND result
//! are NOT pure popcount (the truncation can zero live high bits
//! mid-loop) and are deliberately never matched — only the pair-free
//! shape fires, which is exactly the shape the interval analysis
//! certifies on the hot path.
//!
//! Runs right after `sext_elide` (which produces the pair-free hot
//! shape), before the SSA dump / rc_peephole.
//! `TORAJS_CTPOP_OFF=1` skips (bisect gate),
//! `TORAJS_CTPOP_STATS=1` dumps counters.

use std::collections::HashMap;

use torajs_core::ast::ExprId;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo,
};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CtpopStats {
    /// Kernighan loops replaced with a single `ctpop`.
    pub loops_rewritten: u32,
}

/// Run popcount-idiom recognition over every function body.
pub fn recognize_ctpop_loops(module: &mut Module) -> CtpopStats {
    let mut stats = CtpopStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        recognize_in_function(func, &mut stats);
    }
    stats
}

/// One matched loop, fully resolved to the pieces the rewrite needs.
struct MatchedLoop {
    header: BlockId,
    body: BlockId,
    exit: BlockId,
    n_cell: ValueId,
    c_cell: ValueId,
    origin: Option<ExprId>,
    /// `n_cell` is read outside the loop, so the rewrite must
    /// materialize its post-loop value (always 0).
    n_used_after: bool,
}

fn recognize_in_function(func: &mut Function, stats: &mut CtpopStats) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let mut matches: Vec<MatchedLoop> = Vec::new();
    for lp in la.loops() {
        if lp.body.len() != 2 || lp.back_edge_sources.len() != 1 {
            continue;
        }
        let header = lp.header;
        let body = lp.back_edge_sources[0];
        if body == header || !lp.body.contains(&body) {
            continue;
        }
        if let Some(m) = match_loop(func, header, body) {
            matches.push(m);
        }
    }
    for m in matches {
        rewrite_loop(func, &m);
        stats.loops_rewritten += 1;
    }
}

/// Count defs of each ValueId across the whole function (multi-def
/// Copy cells count once per def site).
fn def_counts(func: &Function) -> HashMap<ValueId, u32> {
    let mut counts: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                *counts.entry(r).or_insert(0) += 1;
            }
        }
    }
    counts
}

fn match_loop(func: &Function, header: BlockId, body: BlockId) -> Option<MatchedLoop> {
    let hb = func.blocks.iter().find(|b| b.id == header)?;
    let bb = func.blocks.iter().find(|b| b.id == body)?;

    // header: exactly `%cmp = icmp ne %n, 0` + `cond_br %cmp, body, exit`.
    if hb.insts.len() != 1 {
        return None;
    }
    let cmp_inst = &hb.insts[0];
    let cmp = cmp_inst.result?;
    let InstKind::ICmp(IPred::Ne, Operand::Value(n_cell), Operand::ConstI64(0)) = &cmp_inst.kind
    else {
        return None;
    };
    let n_cell = *n_cell;
    let Terminator::CondBr {
        cond: Operand::Value(cond),
        then_blk,
        else_blk,
    } = &hb.term
    else {
        return None;
    };
    if *cond != cmp || *then_blk != body || *else_blk == header || *else_blk == body {
        return None;
    }
    let exit = *else_blk;

    // body: exactly the 5 Kernighan insts + `br header`. Resolve from
    // the cell inward: the copy into `n` names the AND, the AND names
    // the SUB; the remaining pair must be `add %c, 1` + its copy.
    if !matches!(bb.term, Terminator::Br(b) if b == header) || bb.insts.len() != 5 {
        return None;
    }
    let (copy_n_idx, m) = bb.insts.iter().enumerate().find_map(|(i, inst)| {
        if inst.result == Some(n_cell) {
            if let InstKind::Copy(Type::I64, Operand::Value(src)) = inst.kind {
                return Some((i, src));
            }
        }
        None
    })?;
    let (and_idx, and_inst) = bb
        .insts
        .iter()
        .enumerate()
        .find(|(_, inst)| inst.result == Some(m))?;
    let InstKind::BinOp(BinOp::And, a, b) = &and_inst.kind else {
        return None;
    };
    // `and(n, t)` in either operand order; the other side is the sub.
    let t = match (*a, *b) {
        (Operand::Value(x), Operand::Value(y)) if x == n_cell && y != n_cell => y,
        (Operand::Value(x), Operand::Value(y)) if y == n_cell && x != n_cell => x,
        _ => return None,
    };
    let (sub_idx, sub_inst) = bb
        .insts
        .iter()
        .enumerate()
        .find(|(_, inst)| inst.result == Some(t))?;
    if !matches!(
        sub_inst.kind,
        InstKind::BinOp(BinOp::Sub, Operand::Value(x), Operand::ConstI64(1)) if x == n_cell
    ) {
        return None;
    }
    let mut rest = (0..5).filter(|i| ![copy_n_idx, and_idx, sub_idx].contains(i));
    let (r0, r1) = (rest.next()?, rest.next()?);
    let (add_idx, copy_c_idx) = if matches!(bb.insts[r0].kind, InstKind::Copy(_, _)) {
        (r1, r0)
    } else {
        (r0, r1)
    };
    let c2 = bb.insts[add_idx].result?;
    let copy_c = &bb.insts[copy_c_idx];
    let c_cell = copy_c.result?;
    if !matches!(
        copy_c.kind,
        InstKind::Copy(Type::I64, Operand::Value(src)) if src == c2
    ) {
        return None;
    }
    let count_up = matches!(
        bb.insts[add_idx].kind,
        InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::ConstI64(1))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::Value(x)) if x == c_cell
    );
    if !count_up || c_cell == n_cell {
        return None;
    }

    // Global side conditions. The 4 intermediates must be single-def
    // (a second def elsewhere would mean the id is a shared cell) and
    // must not be read outside the loop — otherwise deleting the loop
    // leaves readers of a stale value. Every inst in header/body is
    // part of the match, so "outside the loop" = every other block.
    let intermediates = [cmp, t, m, c2];
    let counts = def_counts(func);
    if intermediates.iter().any(|v| counts.get(v) != Some(&1)) {
        return None;
    }
    let mut escaped = false;
    let mut n_used_after = false;
    for block in &func.blocks {
        if block.id == header || block.id == body {
            continue;
        }
        let mut check = |v: ValueId| {
            if intermediates.contains(&v) {
                escaped = true;
            }
            if v == n_cell {
                n_used_after = true;
            }
        };
        for inst in &block.insts {
            visit_value_operands(&inst.kind, &mut check);
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => check(*v),
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => check(*v),
            _ => {}
        }
    }
    if escaped {
        return None;
    }

    Some(MatchedLoop {
        header,
        body,
        exit,
        n_cell,
        c_cell,
        origin: cmp_inst.origin,
        n_used_after,
    })
}

fn push_i64_value(values: &mut Vec<ValueInfo>) -> ValueId {
    let id = ValueId(values.len() as u32);
    values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    id
}

fn rewrite_loop(func: &mut Function, m: &MatchedLoop) {
    let pc = push_i64_value(&mut func.values);
    let sum = push_i64_value(&mut func.values);
    let mut insts = vec![
        Inst {
            result: Some(pc),
            kind: InstKind::Ctpop(Operand::Value(m.n_cell)),
            origin: m.origin,
        },
        Inst {
            result: Some(sum),
            kind: InstKind::BinOp(BinOp::Add, Operand::Value(m.c_cell), Operand::Value(pc)),
            origin: m.origin,
        },
        Inst {
            result: Some(m.c_cell),
            kind: InstKind::Copy(Type::I64, Operand::Value(sum)),
            origin: m.origin,
        },
    ];
    if m.n_used_after {
        // the loop exits when n reaches 0; keep the cell coherent for
        // any post-loop reader.
        insts.push(Inst {
            result: Some(m.n_cell),
            kind: InstKind::Copy(Type::I64, Operand::ConstI64(0)),
            origin: m.origin,
        });
    }
    for block in func.blocks.iter_mut() {
        if block.id == m.header {
            block.insts = std::mem::take(&mut insts);
            block.term = Terminator::Br(m.exit);
        } else if block.id == m.body {
            block.insts.clear();
            block.term = Terminator::Unreachable;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    /// The canonical 4-block popcount shape:
    ///   bb0 (preheader): %0=n_init, n(%1)=copy %0, c(%2)=copy 0 → bb1
    ///   bb1 (header):    %3=icmp ne %1,0; cond_br %3, bb2, bb3
    ///   bb2 (body):      %4=sub %1,1; %5=and %1,%4; %6=add %2,1;
    ///                    %1=copy %5; %2=copy %6 → bb1
    ///   bb3 (exit):      ret %2
    fn popcount_module() -> Module {
        let values = (0..7)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect();
        let blocks = vec![
            torajs_core::ssa::Block {
                id: BlockId(0),
                insts: vec![
                    inst(0, InstKind::BinOp(BinOp::Add, c(100), c(3))),
                    inst(1, InstKind::Copy(Type::I64, v(0))),
                    inst(2, InstKind::Copy(Type::I64, c(0))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            torajs_core::ssa::Block {
                id: BlockId(1),
                insts: vec![inst(3, InstKind::ICmp(IPred::Ne, v(1), c(0)))],
                term: Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            torajs_core::ssa::Block {
                id: BlockId(2),
                insts: vec![
                    inst(4, InstKind::BinOp(BinOp::Sub, v(1), c(1))),
                    inst(5, InstKind::BinOp(BinOp::And, v(1), v(4))),
                    inst(6, InstKind::BinOp(BinOp::Add, v(2), c(1))),
                    inst(1, InstKind::Copy(Type::I64, v(5))),
                    inst(2, InstKind::Copy(Type::I64, v(6))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            torajs_core::ssa::Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(Some(v(2))),
            },
        ];
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::I64,
                blocks,
                values,
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn kernighan_loop_rewrites_to_ctpop() {
        let mut m = popcount_module();
        let stats = recognize_ctpop_loops(&mut m);
        assert_eq!(stats.loops_rewritten, 1);
        let f = &m.funcs[0];
        // header: ctpop + add + copy-into-c, then br exit.
        let header = &f.blocks[1];
        assert_eq!(header.insts.len(), 3);
        assert!(matches!(
            header.insts[0].kind,
            InstKind::Ctpop(Operand::Value(ValueId(1)))
        ));
        assert!(matches!(
            header.insts[1].kind,
            InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(2)), Operand::Value(_))
        ));
        assert_eq!(header.insts[2].result, Some(ValueId(2)));
        assert!(matches!(header.term, Terminator::Br(BlockId(3))));
        // body is dead.
        assert!(f.blocks[2].insts.is_empty());
        assert!(matches!(f.blocks[2].term, Terminator::Unreachable));
    }

    #[test]
    fn n_used_after_loop_materializes_zero() {
        let mut m = popcount_module();
        // exit returns n instead of c — n must be re-materialized as 0.
        m.funcs[0].blocks[3].term = Terminator::Ret(Some(v(1)));
        let stats = recognize_ctpop_loops(&mut m);
        assert_eq!(stats.loops_rewritten, 1);
        let header = &m.funcs[0].blocks[1];
        assert_eq!(header.insts.len(), 4);
        assert!(matches!(
            header.insts[3].kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(0))
        ));
        assert_eq!(header.insts[3].result, Some(ValueId(1)));
    }

    #[test]
    fn and_operands_swapped_still_fires() {
        let mut m = popcount_module();
        m.funcs[0].blocks[2].insts[1] = inst(5, InstKind::BinOp(BinOp::And, v(4), v(1)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 1);
    }

    #[test]
    fn extra_inst_in_body_blocks_fire() {
        let mut m = popcount_module();
        m.funcs[0].blocks[2]
            .insts
            .push(inst(0, InstKind::BinOp(BinOp::Add, c(1), c(1))));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn wrong_sub_amount_blocks_fire() {
        let mut m = popcount_module();
        m.funcs[0].blocks[2].insts[0] = inst(4, InstKind::BinOp(BinOp::Sub, v(1), c(2)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn wrong_count_step_blocks_fire() {
        let mut m = popcount_module();
        m.funcs[0].blocks[2].insts[2] = inst(6, InstKind::BinOp(BinOp::Add, v(2), c(2)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn icmp_eq_blocks_fire() {
        let mut m = popcount_module();
        m.funcs[0].blocks[1].insts[0] = inst(3, InstKind::ICmp(IPred::Eq, v(1), c(0)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn sext_pair_variant_blocks_fire() {
        // A surviving shl/ashr pair on the AND result (the cold-path
        // shape) makes the body 7 insts — must not match: the
        // truncating variant is not a pure popcount.
        let mut m = popcount_module();
        let f = &mut m.funcs[0];
        for _ in 0..2 {
            f.values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
        }
        f.blocks[2].insts = vec![
            inst(4, InstKind::BinOp(BinOp::Sub, v(1), c(1))),
            inst(5, InstKind::BinOp(BinOp::And, v(1), v(4))),
            inst(7, InstKind::BinOp(BinOp::Shl, v(5), c(32))),
            inst(8, InstKind::BinOp(BinOp::AShr, v(7), c(32))),
            inst(6, InstKind::BinOp(BinOp::Add, v(2), c(1))),
            inst(1, InstKind::Copy(Type::I64, v(8))),
            inst(2, InstKind::Copy(Type::I64, v(6))),
        ];
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn intermediate_read_outside_loop_blocks_fire() {
        // exit returns the AND result %5 — deleting the loop would
        // leave the ret reading a stale single-def value.
        let mut m = popcount_module();
        m.funcs[0].blocks[3].term = Terminator::Ret(Some(v(5)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }

    #[test]
    fn count_cell_step_reading_other_cell_blocks_fire() {
        // `%6 = add %1, 1` (reads n, not c) — not a count-up.
        let mut m = popcount_module();
        m.funcs[0].blocks[2].insts[2] = inst(6, InstKind::BinOp(BinOp::Add, v(1), c(1)));
        assert_eq!(recognize_ctpop_loops(&mut m).loops_rewritten, 0);
    }
}
