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

mod arm;
pub mod cmp_sink;
mod range_fold;

use torajs_core::ssa::{BlockId, Function, Inst, InstKind, Module, Operand, Terminator, ValueId};

use arm::{ArmPlan, arm_chain, plan_arm, reachable, switch_value};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectFormStats {
    /// pure diamonds fully converted (CondBr → Br + selects).
    pub diamonds_converted: u32,
    /// Select instructions formed (one per φ-merged join value; the
    /// guard-reduction Selects are plumbing and not counted here).
    pub selects_formed: u32,
    /// non-Copy arm instructions hoisted into the header.
    pub insts_hoisted: u32,
    /// short-circuits of two compares folded to one range test.
    pub range_tests_formed: u32,
}

pub fn form_selects(module: &mut Module) -> SelectFormStats {
    let mut stats = SelectFormStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        // converting an inner diamond can turn its join into a pure
        // arm of an outer one — iterate to a fixpoint (each round
        // that reports a change strictly removed a CondBr, so this
        // terminates).
        while form_once(func, &mut stats) {}
    }
    stats
}

/// What the conversions of one round rewrote: the counts the round
/// started from are stale for these, so a candidate reading any of
/// them waits for the next round, whose counts see the rewrite.
struct Touched {
    blocks: Vec<bool>,
    values: Vec<bool>,
}

impl Touched {
    fn new(func: &Function) -> Self {
        Self {
            blocks: vec![false; func.blocks.len()],
            values: vec![false; func.values.len()],
        }
    }

    /// A value made this round (past the round's start) counts as
    /// rewritten.
    fn any(&self, blocks: &[usize], values: impl IntoIterator<Item = ValueId>) -> bool {
        blocks.iter().any(|&b| self.blocks[b])
            || values
                .into_iter()
                .any(|v| self.values.get(v.0 as usize).is_none_or(|&t| t))
    }

    fn mark(&mut self, blocks: &[usize], values: impl IntoIterator<Item = ValueId>) {
        for &b in blocks {
            self.blocks[b] = true;
        }
        for v in values {
            if let Some(t) = self.values.get_mut(v.0 as usize) {
                *t = true;
            }
        }
    }
}

/// The values whose def counts an arm plan was read against: its
/// copies' targets, and the copies it hoists as single-def ones.
fn plan_cells(plan: &ArmPlan) -> impl Iterator<Item = ValueId> + '_ {
    let hoisted = plan
        .hoisted
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Copy(..)))
        .filter_map(|i| i.result);
    plan.copies.iter().map(|(v, _, _)| *v).chain(hoisted)
}

/// Convert every pure diamond whose blocks and cells no earlier
/// conversion of this round rewrote; report whether any converted.
fn form_once(func: &mut Function, stats: &mut SelectFormStats) -> bool {
    // predecessor counts + global def counts (multi-def Copy check),
    // by block position and value id
    let mut preds = vec![0u32; func.blocks.len()];
    let mut defs = vec![0u32; func.values.len()];
    // a converted diamond's emptied arms still `br` to its join; counted,
    // they would keep that join from ever being part of an outer arm
    let live = reachable(func);
    for block in func.blocks.iter().filter(|b| live[b.id.0 as usize]) {
        match &block.term {
            Terminator::Br(t) => preds[t.0 as usize] += 1,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                preds[then_blk.0 as usize] += 1;
                preds[else_blk.0 as usize] += 1;
            }
            _ => {}
        }
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs[r.0 as usize] += 1;
            }
        }
    }
    let pred_count = |b: BlockId| preds.get(b.0 as usize).copied().unwrap_or(0);
    let def_count = |v: ValueId| defs.get(v.0 as usize).copied().unwrap_or(0);
    let mut touched = Touched::new(func);
    let mut changed = false;

    for h in (0..func.blocks.len()).filter(|&h| live[h]) {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = &func.blocks[h].term
        else {
            continue;
        };
        let (cond, t_id, e_id) = (*cond, *then_blk, *else_blk);
        if t_id == e_id || t_id == func.blocks[h].id || e_id == func.blocks[h].id {
            continue;
        }
        let counts = (&pred_count, &def_count);
        changed |= try_form_pure(func, h, &cond, t_id, e_id, counts, &mut touched, stats);
    }
    changed
}

/// Blade-2 pure diamond: both arms single-pred straight-line chains,
/// all-pure, same join.
#[allow(clippy::too_many_arguments)]
fn try_form_pure(
    func: &mut Function,
    h: usize,
    cond: &Operand,
    t_id: BlockId,
    e_id: BlockId,
    (pred_count, def_count): (&impl Fn(BlockId) -> u32, &impl Fn(ValueId) -> u32),
    touched: &mut Touched,
    stats: &mut SelectFormStats,
) -> bool {
    if pred_count(t_id) != 1 || pred_count(e_id) != 1 {
        return false;
    }
    let (Some((t_blks, t_join)), Some((e_blks, e_join))) = (
        arm_chain(func, t_id, pred_count),
        arm_chain(func, e_id, pred_count),
    ) else {
        return false;
    };
    if t_join != e_join || t_join == func.blocks[h].id {
        return false;
    }
    let join = t_join;
    let on = switch_value(func, h, cond);
    let plan = |blks: &[usize]| plan_arm(func, blks, on, def_count);
    let (Some(tp), Some(ep)) = (plan(&t_blks), plan(&e_blks)) else {
        return false;
    };
    // join values must pair up exactly: same vid set, two defs
    // globally (one per arm), and no copy source feeding on
    // another join value of this same diamond (parallel-copy
    // sequencing across arms is not reconstructed here). A range
    // test needs only one def per arm: a cell with more defs
    // elsewhere (a versioned loop's clone repeats the diamond) keeps
    // them, and the test is a win where a select may not be.
    if tp.copies.len() != ep.copies.len() {
        return false;
    }
    let paired = tp
        .copies
        .iter()
        .all(|(v, _, _)| ep.copies.iter().any(|(w, _, _)| w == v) && def_count(*v) >= 2);
    let self_ref = tp.copies.iter().chain(ep.copies.iter()).any(
        |(_, _, src)| matches!(src, Operand::Value(x) if tp.copies.iter().any(|(v, _, _)| v == x)),
    );
    if !paired || self_ref {
        return false;
    }
    let blocks: Vec<usize> = [h, join.0 as usize]
        .into_iter()
        .chain(t_blks.iter().chain(&e_blks).copied())
        .collect();
    if touched.any(&blocks, plan_cells(&tp).chain(plan_cells(&ep))) {
        return false;
    }
    // convert: hoist arms, form selects (or the one range test the
    // two compares of a short-circuit fold into), collapse the branch
    let fold = range_fold::fold(func, h, cond, &tp, &ep);
    if fold.is_none() && tp.copies.iter().any(|(v, _, _)| def_count(*v) != 2) {
        return false;
    }
    // the two copies of each join value become one def
    touched.mark(&blocks, tp.copies.iter().map(|(v, _, _)| *v));
    let retired = |i: &Inst| {
        fold.as_ref()
            .is_some_and(|f| i.result.is_some_and(|r| f.dead.contains(&r)))
    };
    let mut appended: Vec<Inst> = Vec::new();
    appended.extend(tp.hoisted.into_iter().filter(|i| !retired(i)));
    appended.extend(ep.hoisted.into_iter().filter(|i| !retired(i)));
    stats.insts_hoisted += appended.len() as u32;
    if let Some(f) = fold {
        func.blocks[h].insts.retain(|i| i.result != Some(f.dead[0]));
        appended.extend(f.insts);
        stats.range_tests_formed += 1;
        func.blocks[h].insts.extend(appended);
        func.blocks[h].term = Terminator::Br(join);
        for b in t_blks.into_iter().chain(e_blks) {
            func.blocks[b].insts.clear();
        }
        stats.diamonds_converted += 1;
        return true;
    }
    for (vid, ty, t_src) in &tp.copies {
        let (_, _, e_src) = ep
            .copies
            .iter()
            .find(|(w, _, _)| w == vid)
            .expect("pairing verified above");
        // both arms merge the same value: no choice left to make
        let kind = if t_src == e_src {
            InstKind::Copy(*ty, *t_src)
        } else {
            stats.selects_formed += 1;
            InstKind::Select(*ty, *cond, *t_src, *e_src)
        };
        appended.push(Inst {
            result: Some(*vid),
            kind,
            origin: None,
        });
    }
    func.blocks[h].insts.extend(appended);
    func.blocks[h].term = Terminator::Br(join);
    for b in t_blks.into_iter().chain(e_blks) {
        func.blocks[b].insts.clear();
    }
    stats.diamonds_converted += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, FuncId, IPred, Type, ValueInfo};

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
    fn arms_merging_one_value_form_a_copy() {
        let mut f = max_diamond();
        f.blocks[2].insts[0].kind = InstKind::Copy(Type::I64, c(7));
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!((stats.diamonds_converted, stats.selects_formed), (1, 0));
        assert_eq!(
            f.blocks[0].insts.last().unwrap().kind,
            InstKind::Copy(Type::I64, c(7))
        );
    }

    /// collatz after float_demote: the odd arm computes in one block
    /// and copies in the next (the split a folded guard left behind).
    fn chained_arm_diamond() -> Function {
        func(
            6,
            vec![
                blk(
                    0,
                    vec![inst(1, InstKind::ICmp(IPred::Eq, v(0), c(0)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::AShr, v(0), c(1))),
                        inst(2, InstKind::Copy(Type::I64, v(3))),
                    ],
                    Terminator::Br(BlockId(4)),
                ),
                blk(
                    2,
                    vec![inst(4, InstKind::BinOp(BinOp::Mul, c(3), v(0)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    3,
                    vec![
                        inst(5, InstKind::BinOp(BinOp::Add, v(4), c(1))),
                        inst(2, InstKind::Copy(Type::I64, v(5))),
                    ],
                    Terminator::Br(BlockId(4)),
                ),
                blk(4, vec![], Terminator::Ret(Some(v(2)))),
            ],
        )
    }

    #[test]
    fn chained_arm_forms_select() {
        let mut f = chained_arm_diamond();
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        assert_eq!(stats.insts_hoisted, 3);
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(4))));
        assert_eq!(
            f.blocks[0].insts.last().unwrap().kind,
            InstKind::Select(Type::I64, v(1), v(3), v(5))
        );
        assert!((1..4).all(|b| f.blocks[b].insts.is_empty()));
    }

    #[test]
    fn arm_reading_its_own_copy_keeps_the_branch() {
        // a single-block arm: `%2 = copy …` then an op reading %2
        let mut f = chained_arm_diamond();
        f.blocks[1].insts = vec![
            inst(2, InstKind::Copy(Type::I64, c(9))),
            inst(3, InstKind::BinOp(BinOp::AShr, v(2), c(1))),
        ];
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 0);
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

    /// `if (c) { const t = d; s = s + t }`: the arm's own single-def
    /// copy is a plain value, not a join value the other arm lacks.
    #[test]
    fn single_def_copy_in_arm_hoists() {
        // %0 s, %1 d, %2 c, %3 t, %4 s+t, %5 the merged s
        let mut f = func(
            6,
            vec![
                blk(
                    0,
                    vec![inst(2, InstKind::ICmp(IPred::Slt, v(1), c(9)))],
                    Terminator::CondBr {
                        cond: v(2),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(
                    1,
                    vec![
                        inst(3, InstKind::Copy(Type::I64, v(1))),
                        inst(4, InstKind::BinOp(BinOp::Add, v(0), v(3))),
                        inst(5, InstKind::Copy(Type::I64, v(4))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                blk(
                    2,
                    vec![inst(5, InstKind::Copy(Type::I64, v(0)))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(3, vec![], Terminator::Ret(Some(v(5)))),
            ],
        );
        let mut stats = SelectFormStats::default();
        while form_once(&mut f, &mut stats) {}
        assert_eq!(stats.diamonds_converted, 1);
        let h = &f.blocks[0].insts;
        assert_eq!(h[1].kind, InstKind::Copy(Type::I64, v(1)));
        assert_eq!(h[2].kind, InstKind::BinOp(BinOp::Add, v(0), v(3)));
        assert_eq!(h[3].kind, InstKind::Select(Type::I64, v(2), v(4), v(0)));
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

    /// `m = x === 40 ? 0 : x === 41 ? 1 : … : 99` as lowering leaves it:
    /// level i is header 3i (`%1i = icmp eq x, 40+i`), case 3i+1
    /// (`%2i = copy i`) and join 3i+2 (`%2(i-1) = copy %2i`), the last
    /// block is the default. Level `odd` tests %1 instead of %0.
    fn switch_chain(tests: u32, odd: Option<u32>) -> Function {
        let mut blocks = Vec::new();
        for i in 0..tests {
            let x = if odd == Some(i) { v(1) } else { v(0) };
            blocks.push(blk(
                3 * i,
                vec![inst(
                    10 + i,
                    InstKind::ICmp(IPred::Eq, x, c(40 + i64::from(i))),
                )],
                Terminator::CondBr {
                    cond: v(10 + i),
                    then_blk: BlockId(3 * i + 1),
                    else_blk: BlockId(3 * i + 3),
                },
            ));
            blocks.push(blk(
                3 * i + 1,
                vec![inst(20 + i, InstKind::Copy(Type::I64, c(i64::from(i))))],
                Terminator::Br(BlockId(3 * i + 2)),
            ));
            blocks.push(if i == 0 {
                blk(2, vec![], Terminator::Ret(Some(v(20))))
            } else {
                blk(
                    3 * i + 2,
                    vec![inst(20 + i - 1, InstKind::Copy(Type::I64, v(20 + i)))],
                    Terminator::Br(BlockId(3 * i - 1)),
                )
            });
        }
        blocks.push(blk(
            3 * tests,
            vec![inst(20 + tests - 1, InstKind::Copy(Type::I64, c(99)))],
            Terminator::Br(BlockId(3 * tests - 1)),
        ));
        func(40, blocks)
    }

    fn converted(f: &mut Function) -> u32 {
        let mut stats = SelectFormStats::default();
        while form_once(f, &mut stats) {}
        stats.diamonds_converted
    }

    #[test]
    fn a_switch_picking_constants_forms_a_select_chain() {
        let mut f = switch_chain(4, None);
        assert_eq!(converted(&mut f), 4);
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(2))));
        let h = &f.blocks[0].insts;
        assert_eq!(h.len(), 8);
        assert_eq!(h[7].kind, InstKind::Select(Type::I64, v(10), c(0), v(21)));
    }

    #[test]
    fn a_switch_longer_than_the_cap_keeps_its_outer_tests() {
        let mut f = switch_chain(5, None);
        assert_eq!(converted(&mut f), 4);
        assert!(matches!(f.blocks[0].term, Terminator::CondBr { .. }));
        assert!(matches!(f.blocks[3].term, Terminator::Br(BlockId(5))));
    }

    #[test]
    fn a_select_under_a_test_of_another_value_keeps_the_branch() {
        let mut f = switch_chain(3, Some(1));
        assert_eq!(converted(&mut f), 1);
        assert!(matches!(f.blocks[3].term, Terminator::CondBr { .. }));
    }
}
