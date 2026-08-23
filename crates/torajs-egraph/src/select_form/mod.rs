//! Select formation — if-conversion of pure CondBr diamonds into
//! `InstKind::Select` (LLVM SimplifyCFG's SpeculativelyExecuteBB +
//! FoldTwoEntryPHINode analogue; RFC 20260719-select-formation blade 2).
//!
//! Shape: a header ending in `CondBr{cond, T, E}` where T and E are
//! single-predecessor blocks that both `Br` to the same join, and every
//! arm instruction is either (a) a trap-free, effect-free def from the
//! speculation whitelist, or (b) a multi-def `Copy` materializing one
//! φ-merged join value (phi_promote's destruction product — exactly two
//! defs, one per arm). The arms hoist into the header, each Copy pair
//! becomes `%m = select cond, a, b`, and the CondBr collapses to `Br
//! join`. The emptied arm blocks stay as `Br` forwarders — codegen's
//! brfuse `resolve_forwarded_target` folds them to zero machine code.
//!
//! Why: an unpredictable data-dependent branch costs mispredicts that
//! csel eliminates outright. Both arms ALWAYS execute after
//! conversion, hence the whitelist: no calls (incl. FRem's libm BL),
//! no loads (trap), no stores, and arm size capped so speculation
//! can't out-cost the branch it replaces.
//!
//! Blade 3 (guarded-arm speculation over float_demote's window-guard
//! chains) was RETIRED in S7 (rotation 484, 2026-08-24): a full-bench
//! survey showed collatz was its only real hit, and a same-HEAD
//! ablation measured the conversion as a 9.5% LOSS there — the
//! never-taken side-exit guards are perfectly predicted branches
//! (free), while the Select cascade it built put ~4 cycles of
//! cset/csel latency onto the loop-carried n chain. Its founding
//! premise ("collatz parity ≈ coin flip") undercounted the structural
//! predictability (odd → 3n+1 is always even) that a modern TAGE
//! predictor exploits. Deopt-style guards want branches, not selects.
//!
//! Runs after ctpop_idiom (all arm-shaping rewrites — float_demote /
//! srem_parity — are done) and before rc_peephole, which treats Select
//! as rc-transparent via `classify_pure`. `TORAJS_SELECT_FORM_OFF=1`
//! skips, `TORAJS_SELECT_FORM_STATS=1` dumps counters.

pub mod cmp_sink;

use std::collections::HashMap;

use torajs_core::ssa::{BinOp, BlockId, Function, Inst, InstKind, Module, Operand, Terminator};
use torajs_core::ssa::{Type, ValueId};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectFormStats {
    /// pure diamonds fully converted (CondBr → Br + selects).
    pub diamonds_converted: u32,
    /// Select instructions formed (one per φ-merged join value; the
    /// guard-reduction Selects are plumbing and not counted here).
    pub selects_formed: u32,
    /// non-Copy arm instructions hoisted into the header.
    pub insts_hoisted: u32,
}

/// Per-arm speculation cap: at most this many non-Copy defs may hoist.
/// A csel replaces one branch (~1 cyc taken + mispredict tail); two
/// ALU ops of straight-line speculation is well under that budget,
/// fatter arms keep their branch. Bench-tunable (blade 2 validation).
const MAX_SPECULATED_INSTS: usize = 2;

pub fn form_selects(module: &mut Module) -> SelectFormStats {
    let mut stats = SelectFormStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        // converting an inner diamond can turn its join into a pure
        // arm of an outer one — iterate to a fixpoint (each round
        // strictly removes a CondBr, so this terminates).
        while form_once(func, &mut stats) {}
    }
    stats
}

/// Instructions safe to execute unconditionally: no memory access, no
/// calls, no trap. NOT `optimize::is_pure` — that is GVN purity, which
/// admits FRem (lowers to a libm `_fmod` BL, regalloc.rs
/// `inst_emits_bl`) and is the wrong question for Load (GVN-impure yet
/// also unsafe to hoist past its guard). SDiv/SRem are trap-free on
/// aarch64 but occupy the divider for ~10 cyc — outside the budget.
pub(super) fn can_speculate(kind: &InstKind) -> bool {
    match kind {
        InstKind::BinOp(op, _, _) => !matches!(op, BinOp::FRem | BinOp::SDiv | BinOp::SRem),
        InstKind::ICmp(_, _, _)
        | InstKind::FCmp(_, _, _)
        | InstKind::Neg(_)
        | InstKind::Ctpop(_)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_)
        | InstKind::BitCastF64ToI64(_)
        | InstKind::BitCastI64ToF64(_)
        | InstKind::IntToPtr(_)
        | InstKind::PtrToInt(_)
        | InstKind::TruncI64ToBool(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_) => true,
        _ => false,
    }
}

/// One arm's dissection: hoistable defs in order, plus the join Copys
/// in order as `(vid, ty, src)`.
pub(super) struct ArmPlan {
    pub(super) hoisted: Vec<Inst>,
    pub(super) copies: Vec<(ValueId, Type, Operand)>,
}

/// Dissect one arm block. `None` = arm not convertible.
pub(super) fn plan_arm(func: &Function, blk: usize) -> Option<ArmPlan> {
    let mut hoisted = Vec::new();
    let mut copies: Vec<(ValueId, Type, Operand)> = Vec::new();
    for inst in &func.blocks[blk].insts {
        match &inst.kind {
            InstKind::Copy(ty, src) => {
                let vid = inst.result.expect("Copy always defines a value");
                if copies.iter().any(|(v, _, _)| *v == vid) {
                    return None; // duplicate def in one arm — not our shape
                }
                copies.push((vid, *ty, src.clone()));
            }
            kind if can_speculate(kind) && inst.result.is_some() => {
                hoisted.push(inst.clone());
            }
            _ => return None,
        }
    }
    if hoisted.len() > MAX_SPECULATED_INSTS {
        return None;
    }
    Some(ArmPlan { hoisted, copies })
}

/// Scan for one convertible pure diamond; convert it
/// and report `true`, or return `false` when none is left.
fn form_once(func: &mut Function, stats: &mut SelectFormStats) -> bool {
    // predecessor counts + global def counts (multi-def Copy check)
    let mut preds: HashMap<BlockId, u32> = HashMap::new();
    let mut defs: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        match &block.term {
            Terminator::Br(t) => *preds.entry(*t).or_insert(0) += 1,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                *preds.entry(*then_blk).or_insert(0) += 1;
                *preds.entry(*else_blk).or_insert(0) += 1;
            }
            _ => {}
        }
        for inst in &block.insts {
            if let Some(r) = inst.result {
                *defs.entry(r).or_insert(0) += 1;
            }
        }
    }
    let pred_count = |b: BlockId| preds.get(&b).copied().unwrap_or(0);
    let def_count = |v: ValueId| defs.get(&v).copied().unwrap_or(0);

    for h in 0..func.blocks.len() {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = &func.blocks[h].term
        else {
            continue;
        };
        let (cond, t_id, e_id) = (cond.clone(), *then_blk, *else_blk);
        if t_id == e_id || t_id == func.blocks[h].id || e_id == func.blocks[h].id {
            continue;
        }
        if try_form_pure(func, h, &cond, t_id, e_id, &pred_count, &def_count, stats) {
            return true;
        }
    }
    false
}

/// Blade-2 pure diamond: both arms single-pred, all-pure, same join.
#[allow(clippy::too_many_arguments)]
fn try_form_pure(
    func: &mut Function,
    h: usize,
    cond: &Operand,
    t_id: BlockId,
    e_id: BlockId,
    pred_count: impl Fn(BlockId) -> u32,
    def_count: impl Fn(ValueId) -> u32,
    stats: &mut SelectFormStats,
) -> bool {
    // BlockId.0 == position invariant (phi_promote destruct upholds)
    let (t, e) = (t_id.0 as usize, e_id.0 as usize);
    if pred_count(t_id) != 1 || pred_count(e_id) != 1 {
        return false;
    }
    let (Terminator::Br(t_join), Terminator::Br(e_join)) =
        (&func.blocks[t].term, &func.blocks[e].term)
    else {
        return false;
    };
    if t_join != e_join || *t_join == t_id || *t_join == e_id {
        return false;
    }
    let join = *t_join;
    let (Some(tp), Some(ep)) = (plan_arm(func, t), plan_arm(func, e)) else {
        return false;
    };
    // join values must pair up exactly: same vid set, two defs
    // globally (one per arm), and no copy source feeding on
    // another join value of this same diamond (parallel-copy
    // sequencing across arms is not reconstructed here).
    if tp.copies.len() != ep.copies.len() {
        return false;
    }
    let paired = tp
        .copies
        .iter()
        .all(|(v, _, _)| ep.copies.iter().any(|(w, _, _)| w == v) && def_count(*v) == 2);
    let self_ref = tp.copies.iter().chain(ep.copies.iter()).any(
        |(_, _, src)| matches!(src, Operand::Value(x) if tp.copies.iter().any(|(v, _, _)| v == x)),
    );
    if !paired || self_ref {
        return false;
    }

    // convert: hoist arms, form selects, collapse the branch
    let mut appended: Vec<Inst> = Vec::new();
    stats.insts_hoisted += (tp.hoisted.len() + ep.hoisted.len()) as u32;
    appended.extend(tp.hoisted);
    appended.extend(ep.hoisted);
    for (vid, ty, t_src) in &tp.copies {
        let (_, _, e_src) = ep
            .copies
            .iter()
            .find(|(w, _, _)| w == vid)
            .expect("pairing verified above");
        appended.push(Inst {
            result: Some(*vid),
            kind: InstKind::Select(*ty, cond.clone(), t_src.clone(), e_src.clone()),
            origin: None,
        });
        stats.selects_formed += 1;
    }
    func.blocks[h].insts.extend(appended);
    func.blocks[h].term = Terminator::Br(join);
    func.blocks[t].insts.clear();
    func.blocks[e].insts.clear();
    stats.diamonds_converted += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FuncId, IPred, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn cf(n: f64) -> Operand {
        Operand::ConstF64(n)
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

    fn func(values: usize, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values: (0..values)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    /// max-shape: b0 cmp + condbr, b1/b2 copy-only, b3 join.
    fn max_diamond() -> Function {
        func(
            4,
            vec![
                blk(
                    0,
                    vec![inst(0, InstKind::ICmp(IPred::Sgt, c(7), c(5)))],
                    Terminator::CondBr {
                        cond: v(0),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![inst(1, InstKind::Copy(Type::I64, c(7)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    2,
                    vec![inst(1, InstKind::Copy(Type::I64, c(5)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(3, vec![], Terminator::Ret(Some(v(1)))),
            ],
        )
    }

    #[test]
    fn copy_only_diamond_forms_select() {
        let mut f = max_diamond();
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        assert_eq!(stats.selects_formed, 1);
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(3))));
        assert_eq!(
            f.blocks[0].insts.last().unwrap().kind,
            InstKind::Select(Type::I64, v(0), c(7), c(5))
        );
        assert!(f.blocks[1].insts.is_empty() && f.blocks[2].insts.is_empty());
    }

    #[test]
    fn pure_arm_insts_hoist_before_select() {
        // abs-shape: then arm negates before the copy
        let mut f = func(
            4,
            vec![
                blk(
                    0,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), c(0)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![
                        inst(2, InstKind::Neg(v(0))),
                        inst(3, InstKind::Copy(Type::I64, v(2))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    2,
                    vec![inst(3, InstKind::Copy(Type::I64, v(0)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(3, vec![], Terminator::Ret(Some(v(3)))),
            ],
        );
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        assert_eq!(stats.insts_hoisted, 1);
        let h = &f.blocks[0].insts;
        assert!(matches!(h[1].kind, InstKind::Neg(_)));
        assert_eq!(h[2].kind, InstKind::Select(Type::I64, v(1), v(2), v(0)));
    }

    #[test]
    fn call_in_arm_bails() {
        let mut f = max_diamond();
        f.blocks[1]
            .insts
            .insert(0, inst(2, InstKind::Call(FuncId(0), vec![])));
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 0);
    }

    #[test]
    fn third_def_elsewhere_bails() {
        // join value also defined by a third pred's copy — N-way join,
        // not a diamond pair.
        let mut f = max_diamond();
        f.blocks[3]
            .insts
            .push(inst(1, InstKind::Copy(Type::I64, c(9))));
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 0);
    }

    #[test]
    fn f64_join_value_forms_fcsel_select() {
        // f64 arms convert like any other type — register class comes
        // from the value's declared type, so the emitter picks FCSEL
        // while the condition stays on a GPR.
        let mut f = max_diamond();
        f.blocks[1].insts[0] = inst(1, InstKind::Copy(Type::F64, cf(7.0)));
        f.blocks[2].insts[0] = inst(1, InstKind::Copy(Type::F64, cf(5.0)));
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        let h = &f.blocks[0].insts;
        assert_eq!(
            h[h.len() - 1].kind,
            InstKind::Select(Type::F64, v(0), cf(7.0), cf(5.0))
        );
    }

    #[test]
    fn two_join_values_form_two_selects() {
        let mut f = func(
            5,
            vec![
                blk(
                    0,
                    vec![inst(0, InstKind::ICmp(IPred::Sgt, c(7), c(5)))],
                    Terminator::CondBr {
                        cond: v(0),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![
                        inst(1, InstKind::Copy(Type::I64, c(7))),
                        inst(2, InstKind::Copy(Type::I64, c(1))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    2,
                    vec![
                        inst(1, InstKind::Copy(Type::I64, c(5))),
                        inst(2, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                blk(3, vec![], Terminator::Ret(Some(v(1)))),
            ],
        );
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        assert_eq!(stats.selects_formed, 2);
    }
}
