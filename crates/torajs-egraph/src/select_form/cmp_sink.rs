//! Compare sink — move a single-use compare (ICmp or FCmp) down to
//! sit immediately before the Select that consumes it (RFC
//! 20260719-select-formation, "加宽 ICmp 融合窗口" route ③).
//!
//! Codegen's NZCV fuse (`brfuse::fusible_select_cmps`) is adjacency-
//! gated: it re-emits the compare right before the CSEL, which is only
//! sound because zero intervening instructions means nothing can have
//! been assigned the compare operands' physical registers in between.
//! Select formation routinely breaks that adjacency — the speculated
//! arm instructions hoist into the header *after* the ICmp, so the
//! very diamonds the pass converts keep the `cset; cmp #0; csel`
//! three-level carry chain the fuse exists to remove (csinc's A/B put
//! that chain at ~half the loop's wall clock on a counting shape).
//!
//! Widening the fuse window in codegen instead would need register-
//! allocation knowledge (are the compare's operands still readable at
//! the Select?), which `fusible_*` cannot see. Sinking at the SSA
//! layer dissolves the question: liveness is computed after this pass,
//! on the sunk order, so the operands are live to the Select by
//! construction. The only SSA-level hazard is a *redefinition* in the
//! gap — φ-destruction leaves multi-def `Copy`s (loop-carried values
//! redefine on the latch path), so a gap instruction may redefine an
//! ICmp operand; those sinks are rejected.
//!
//! `TORAJS_CMP_SINK_OFF=1` skips, `TORAJS_CMP_SINK_STATS=1` dumps
//! counters (gated_pass convention).

use torajs_core::ssa::{
    BinOp, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use crate::id_hash::HashMap;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CmpSinkStats {
    /// Compares moved to sit immediately before their Select (or the
    /// ADD of a csinc candidate), plus mask ANDs moved next to them.
    pub sunk: u32,
}

pub fn sink_cmps(module: &mut Module) -> CmpSinkStats {
    let mut stats = CmpSinkStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let uses = count_uses(func);
        for b in 0..func.blocks.len() {
            // A sink shifts indices; restart the block scan after each
            // move. A sunk compare lands adjacent and never re-matches,
            // so this terminates.
            while sink_one(func, b, &uses) {
                stats.sunk += 1;
            }
        }
    }
    stats
}

/// Use counts over inst + terminator operands — same accounting as
/// codegen's `brfuse::count_uses`, whose single-use gate this pass
/// front-runs (a cond the fuse would reject is not worth moving).
fn count_uses(func: &Function) -> HashMap<ValueId, u32> {
    let mut uses: HashMap<ValueId, u32> = HashMap::default();
    let mut bump = |v: ValueId| *uses.entry(v).or_insert(0) += 1;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, &mut bump);
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => bump(*v),
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => bump(*v),
            _ => {}
        }
    }
    uses
}

/// Find and perform one sink in block `b`. Returns false when none is
/// left.
fn sink_one(func: &mut Function, b: usize, uses: &HashMap<ValueId, u32>) -> bool {
    let insts = &func.blocks[b].insts;
    for si in 0..insts.len() {
        let InstKind::Select(_, Operand::Value(c), _, _) = &insts[si].kind else {
            continue;
        };
        if uses.get(c) != Some(&1) {
            continue;
        }
        // The defining compare, in this block. A non-compare def or a
        // def in another block is not fusible and not worth moving.
        // FCmp sinks on the same terms — it is equally pure, and the
        // fuse's own gates (FPred::One, the F64 FPR-residency check)
        // decide fusibility after the fact; an unfused-but-adjacent
        // compare emits exactly as it did before.
        let Some(ci) = insts[..si].iter().position(|i| {
            i.result == Some(*c) && matches!(i.kind, InstKind::ICmp(..) | InstKind::FCmp(..))
        }) else {
            continue;
        };
        // the compare goes right before the Select, or right before
        // the ADD of a csinc candidate (selinc absorbs `icmp; add;
        // select`, so sinking between them would displace the ADD)
        let dest = if is_csinc_shape(insts, si, uses) {
            si - 1
        } else {
            si
        };
        if ci + 1 >= dest {
            // in place — pull its mask AND next to it
            let Some(ai) = mask_to_sink(insts, ci, uses) else {
                continue;
            };
            let and = func.blocks[b].insts.remove(ai);
            func.blocks[b].insts.insert(ci - 1, and);
            return true;
        }
        // Redefinition hazard: φ-destructed multi-def Copys may
        // redefine a compare operand (or the cond itself) inside the
        // gap; the delayed compare would read the wrong generation.
        let (InstKind::ICmp(_, lhs, rhs) | InstKind::FCmp(_, lhs, rhs)) = &insts[ci].kind else {
            unreachable!("position() matched a compare");
        };
        let mut pinned: Vec<ValueId> = vec![*c];
        for op in [lhs, rhs] {
            if let Operand::Value(v) = op {
                pinned.push(*v);
            }
        }
        if redefines(&insts[ci + 1..dest], &pinned) {
            continue;
        }
        let cmp = func.blocks[b].insts.remove(ci);
        func.blocks[b].insts.insert(dest - 1, cmp);
        return true;
    }
    false
}

fn redefines(gap: &[torajs_core::ssa::Inst], pinned: &[ValueId]) -> bool {
    gap.iter()
        .any(|i| i.result.is_some_and(|r| pinned.contains(&r)))
}

/// `a = and x, K; …; icmp eq|ne a, 0` with a single-use `a`: the
/// index of the AND to move right before the compare, where codegen
/// folds the pair into one `tst x, #K` (`selinc`). Same redefinition
/// hazard as the compare sink, on `x` and `a`.
fn mask_to_sink(
    insts: &[torajs_core::ssa::Inst],
    ci: usize,
    uses: &HashMap<ValueId, u32>,
) -> Option<usize> {
    let InstKind::ICmp(IPred::Eq | IPred::Ne, Operand::Value(a), Operand::ConstI64(0)) =
        insts[ci].kind
    else {
        return None;
    };
    if uses.get(&a) != Some(&1) {
        return None;
    }
    let ai = insts[..ci].iter().position(|i| i.result == Some(a))?;
    let InstKind::BinOp(BinOp::And, Operand::Value(x), Operand::ConstI64(_)) = insts[ai].kind
    else {
        return None;
    };
    (ai + 1 != ci && !redefines(&insts[ai + 1..ci], &[a, x])).then_some(ai)
}

/// True when the Select is a csinc candidate — `select c, (x+1), y`
/// over I64 with a single-use adjacent ADD (codegen's
/// `selinc::fusible_select_incs`, which absorbs the ADD *and* the
/// compare into one CSINC). Sinking the ICmp between them would
/// displace the ADD and downgrade `cmp; cinc` to `add; cmp; csel`.
fn is_csinc_shape(
    insts: &[torajs_core::ssa::Inst],
    si: usize,
    uses: &HashMap<ValueId, u32>,
) -> bool {
    let InstKind::Select(Type::I64, _, then_op, else_op) = &insts[si].kind else {
        return false;
    };
    if si == 0 {
        return false;
    }
    let add = &insts[si - 1];
    let Some(add_result) = add.result else {
        return false;
    };
    let InstKind::BinOp(BinOp::Add, a_lhs, a_rhs) = &add.kind else {
        return false;
    };
    if !matches!(
        (a_lhs, a_rhs),
        (_, Operand::ConstI64(1)) | (Operand::ConstI64(1), _)
    ) {
        return false;
    }
    if uses.get(&add_result) != Some(&1) {
        return false;
    }
    (*then_op == Operand::Value(add_result)) != (*else_op == Operand::Value(add_result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

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

    fn func_with(insts: Vec<Inst>, term: Terminator) -> Function {
        Function {
            name: "t".into(),
            params: vec![],
            ret: Type::I64,
            values: (0..64)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term,
            }],
            current_origin: None,
        }
    }

    fn module_of(func: Function) -> Module {
        Module {
            funcs: vec![func],
            ..Default::default()
        }
    }

    use torajs_core::ssa::IPred;

    fn icmp(result: u32, lhs: Operand, rhs: Operand) -> Inst {
        inst(result, InstKind::ICmp(IPred::Sgt, lhs, rhs))
    }

    fn add(result: u32, lhs: Operand, rhs: Operand) -> Inst {
        inst(result, InstKind::BinOp(BinOp::Add, lhs, rhs))
    }

    fn select(result: u32, cond: u32, t: Operand, e: Operand) -> Inst {
        inst(result, InstKind::Select(Type::I64, v(cond), t, e))
    }

    #[test]
    fn sinks_across_speculated_gap() {
        // icmp; add; sub; select  →  add; sub; icmp; select
        let f = func_with(
            vec![
                icmp(10, v(1), v(2)),
                add(11, v(3), v(4)),
                inst(12, InstKind::BinOp(BinOp::Sub, v(3), v(4))),
                select(13, 10, v(11), v(12)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        let stats = sink_cmps(&mut m);
        assert_eq!(stats.sunk, 1);
        let insts = &m.funcs[0].blocks[0].insts;
        assert!(matches!(insts[2].kind, InstKind::ICmp(..)));
        assert!(matches!(insts[3].kind, InstKind::Select(..)));
    }

    #[test]
    fn adjacent_pair_is_left_alone() {
        let f = func_with(
            vec![
                add(11, v(3), v(4)),
                icmp(10, v(1), v(2)),
                select(13, 10, v(11), v(3)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 0);
    }

    #[test]
    fn multi_use_cond_is_not_moved() {
        // cond feeds two selects — the fuse would reject it, so the
        // sink leaves it where it is.
        let f = func_with(
            vec![
                icmp(10, v(1), v(2)),
                add(11, v(3), v(4)),
                select(13, 10, v(11), v(3)),
                select(14, 10, v(3), v(11)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 0);
    }

    #[test]
    fn gap_redefinition_blocks_the_sink() {
        // A φ-destructed Copy redefines the compare's lhs inside the
        // gap; the delayed compare would read the wrong generation.
        let f = func_with(
            vec![
                icmp(10, v(1), v(2)),
                inst(1, InstKind::Copy(Type::I64, v(5))),
                select(13, 10, v(5), v(2)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 0);
    }

    #[test]
    fn csinc_shape_is_left_for_selinc() {
        // icmp; add x,1; select c,(x+1),x — codegen's CSINC fuse
        // wants this exact triple; sinking would displace the ADD.
        let f = func_with(
            vec![
                icmp(10, v(1), v(2)),
                add(11, v(3), Operand::ConstI64(1)),
                select(13, 10, v(11), v(3)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 0);
    }

    #[test]
    fn csinc_compare_sinks_to_the_add_and_pulls_its_mask() {
        // collatz after select formation: and; icmp; arms; add; select
        let f = func_with(
            vec![
                inst(9, InstKind::BinOp(BinOp::And, v(1), Operand::ConstI64(1))),
                inst(10, InstKind::ICmp(IPred::Eq, v(9), Operand::ConstI64(0))),
                inst(12, InstKind::BinOp(BinOp::AShr, v(1), Operand::ConstI64(1))),
                inst(14, InstKind::BinOp(BinOp::Mul, Operand::ConstI64(3), v(1))),
                add(11, v(14), Operand::ConstI64(1)),
                select(13, 10, v(12), v(11)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 2);
        let insts = &m.funcs[0].blocks[0].insts;
        assert!(matches!(insts[2].kind, InstKind::BinOp(BinOp::And, ..)));
        assert!(matches!(insts[3].kind, InstKind::ICmp(..)));
        assert!(matches!(insts[4].kind, InstKind::BinOp(BinOp::Add, ..)));
        assert!(matches!(insts[5].kind, InstKind::Select(..)));
    }

    #[test]
    fn fcmp_sinks_like_icmp() {
        use torajs_core::ssa::FPred;
        let f = func_with(
            vec![
                inst(10, InstKind::FCmp(FPred::Olt, v(1), v(2))),
                add(11, v(3), v(4)),
                select(13, 10, v(11), v(3)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 1);
        let insts = &m.funcs[0].blocks[0].insts;
        assert!(matches!(insts[1].kind, InstKind::FCmp(..)));
        assert!(matches!(insts[2].kind, InstKind::Select(..)));
    }

    #[test]
    fn csinc_shape_with_gap_still_sinks() {
        // The ADD is not adjacent to the Select, so selinc cannot
        // claim it — the sink proceeds and the plain fuse fires.
        let f = func_with(
            vec![
                icmp(10, v(1), v(2)),
                add(11, v(3), Operand::ConstI64(1)),
                inst(12, InstKind::BinOp(BinOp::Sub, v(4), v(5))),
                select(13, 10, v(11), v(12)),
            ],
            Terminator::Ret(Some(v(13))),
        );
        let mut m = module_of(f);
        assert_eq!(sink_cmps(&mut m).sunk, 1);
        let insts = &m.funcs[0].blocks[0].insts;
        assert!(matches!(insts[2].kind, InstKind::ICmp(..)));
    }
}
