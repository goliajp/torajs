//! Conditional-increment fuse — `select c, (x+1), x` collapses into a
//! single CSINC, and the compare feeding `c` rides NZCV straight into
//! it.
//!
//! Counting loops (`if (pred) { n = n + 1 }`) reach the backend as the
//! three-instruction tail the select-formation pass leaves behind:
//!
//! ```text
//!   %c = icmp <pred> a, b
//!   %t = add %n, 1
//!   %r = select i64 %c, %t, %n
//! ```
//!
//! which lowers to `cmp; cset; add; cmp #0; csel` — five instructions
//! where `cmp; csinc` does the job. CSINC's `Xm + 1` false-arm is
//! exactly the increment, so the ADD disappears; dropping it also puts
//! the ICmp adjacent to the select, unlocking the same NZCV fuse
//! `brfuse::fusible_select_cmps` performs for plain selects (the ADD
//! is what blocked it before).
//!
//! Both absorptions are gated the same way as the sibling fuses:
//! adjacency (zero intervening instructions, so NZCV stays live) plus
//! single use of the absorbed result (so the skipped ADD's / CSET's
//! value is unobservable).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Type, ValueId};

use super::brfuse::{FusedCmp, count_uses};
use super::cmp::ipred_to_cond;
use crate::enc::cond;

/// One recognized `select c, (x+1), x`.
pub(crate) struct SelectIncPlan {
    /// The value feeding both CSINC sources (`Xn` and `Xm`).
    pub base: Operand,
    /// The select's condition — read only when `cmp` is `None`, where
    /// the emitter falls back to `cmp cond, #0`.
    pub cond: Operand,
    /// The adjacent single-use ICmp absorbed with the ADD; when set,
    /// the compare re-emits directly before the CSINC.
    pub cmp: Option<FusedCmp>,
    /// Condition code for the CSINC. CSINC yields `Xn` when it holds
    /// and `Xm + 1` when it does not, so an increment on the *then*
    /// arm carries the inverted predicate.
    pub cc: u8,
}

/// Recognized plans plus the instruction slots the Select now emits on
/// their behalf (the ADD, and the ICmp when it fused too).
pub(crate) struct SelectIncs {
    plans: HashMap<(u32, u32), SelectIncPlan>,
    absorbed: HashSet<(u32, u32)>,
}

impl SelectIncs {
    pub(crate) fn plan(&self, block: u32, idx: u32) -> Option<&SelectIncPlan> {
        self.plans.get(&(block, idx))
    }

    /// True when the Select downstream emits this instruction, so the
    /// driver's inst loop must skip it.
    pub(crate) fn absorbed(&self, block: u32, idx: u32) -> bool {
        self.absorbed.contains(&(block, idx))
    }
}

/// `x + 1` in either operand order — returns the `x` side.
fn increment_base(kind: &InstKind) -> Option<&Operand> {
    let InstKind::BinOp(BinOp::Add, lhs, rhs) = kind else {
        return None;
    };
    match (lhs, rhs) {
        (base, Operand::ConstI64(1)) => Some(base),
        (Operand::ConstI64(1), base) => Some(base),
        _ => None,
    }
}

pub(crate) fn fusible_select_incs(func: &Function) -> SelectIncs {
    let uses = count_uses(func);
    let mut plans = HashMap::new();
    let mut absorbed = HashSet::new();

    for block in &func.blocks {
        for (ii, inst) in block.insts.iter().enumerate() {
            if ii == 0 {
                continue; // needs a preceding ADD
            }
            // CSINC is the 64-bit X-form; only I64 selects qualify.
            let InstKind::Select(Type::I64, Operand::Value(c), then_op, else_op) = &inst.kind
            else {
                continue;
            };
            let add = &block.insts[ii - 1];
            let Some(add_result) = add.result else {
                continue;
            };
            let Some(base) = increment_base(&add.kind) else {
                continue;
            };
            // Dropping the ADD is only sound when nothing else reads it.
            if uses.get(&add_result) != Some(&1) {
                continue;
            }
            // Which arm is the increment decides the polarity: CSINC
            // increments on the *false* side.
            let inc_on_then = *then_op == Operand::Value(add_result) && else_op == base;
            let inc_on_else = *else_op == Operand::Value(add_result) && then_op == base;
            if !inc_on_then && !inc_on_else {
                continue;
            }

            // Second absorption: an adjacent single-use ICmp defining
            // the condition folds its NZCV straight into the CSINC.
            let fused_cmp = fused_predicate(func, block.id.0, ii, c, &uses);
            let (cond_true_cc, cmp) = match fused_cmp {
                Some((fc, pred_cc)) => (pred_cc, Some(fc)),
                None => (cond::NE, None), // `cmp cond, #0` — NE means "cond is true"
            };
            let cc = if inc_on_then {
                invert_cond(cond_true_cc)
            } else {
                cond_true_cc
            };

            absorbed.insert((block.id.0, ii as u32 - 1));
            if cmp.is_some() {
                absorbed.insert((block.id.0, ii as u32 - 2));
            }
            plans.insert(
                (block.id.0, ii as u32),
                SelectIncPlan {
                    base: *base,
                    cond: Operand::Value(*c),
                    cmp,
                    cc,
                },
            );
        }
    }
    SelectIncs { plans, absorbed }
}

/// The ICmp two slots back, when it defines the select's condition and
/// nothing else reads it. Returns the compare plus the cond code that
/// means "the condition is true".
fn fused_predicate(
    func: &Function,
    block_id: u32,
    select_idx: usize,
    cond_vid: &ValueId,
    uses: &HashMap<ValueId, u32>,
) -> Option<(FusedCmp, u8)> {
    if select_idx < 2 {
        return None;
    }
    let block = func.blocks.iter().find(|b| b.id.0 == block_id)?;
    let candidate = &block.insts[select_idx - 2];
    let InstKind::ICmp(pred, lhs, rhs) = &candidate.kind else {
        return None;
    };
    if candidate.result != Some(*cond_vid) || uses.get(cond_vid) != Some(&1) {
        return None;
    }
    Some((
        FusedCmp {
            pred: super::brfuse::FusedSelPred::Int(*pred),
            lhs: *lhs,
            rhs: *rhs,
        },
        ipred_to_cond(*pred),
    ))
}

/// AArch64 condition codes pair as `cc ^ 1` (EQ/NE, GE/LT, GT/LE, ...).
fn invert_cond(cc: u8) -> u8 {
    cc ^ 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, IPred, Inst, Terminator, ValueInfo};

    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    /// `%0 = icmp eq a, 1 ; %1 = add %n, 1 ; %2 = select %0, %1, %n`
    /// — the counting-loop shape, optionally with an extra reader of
    /// the ADD or of the condition to break each absorption.
    fn count_func(extra_add_use: bool, extra_cond_use: bool) -> Function {
        let vi = |ty: Type| ValueInfo { ty, name: None };
        let mut insts = vec![
            mk(
                0,
                InstKind::ICmp(IPred::Eq, Operand::ConstI64(7), Operand::ConstI64(1)),
            ),
            mk(
                1,
                InstKind::BinOp(BinOp::Add, Operand::ConstI64(5), Operand::ConstI64(1)),
            ),
            mk(
                2,
                InstKind::Select(
                    Type::I64,
                    Operand::Value(ValueId(0)),
                    Operand::Value(ValueId(1)),
                    Operand::ConstI64(5),
                ),
            ),
        ];
        let mut values = vec![vi(Type::Bool), vi(Type::I64), vi(Type::I64)];
        if extra_add_use {
            insts.push(mk(3, InstKind::Copy(Type::I64, Operand::Value(ValueId(1)))));
            values.push(vi(Type::I64));
        }
        if extra_cond_use {
            insts.push(mk(4, InstKind::ZExtBoolToI64(Operand::Value(ValueId(0)))));
            values.push(vi(Type::I64));
        }
        Function {
            name: "count".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
            }],
            current_origin: None,
        }
    }

    #[test]
    fn counting_shape_absorbs_add_and_cmp() {
        let f = count_func(false, false);
        let incs = fusible_select_incs(&f);
        let plan = incs.plan(0, 2).expect("select plan recognized");
        assert_eq!(plan.base, Operand::ConstI64(5));
        assert!(plan.cmp.is_some(), "adjacent single-use icmp must fuse");
        // increment sits on the then arm, so the CSINC carries the
        // inverted predicate: eq -> ne.
        assert_eq!(plan.cc, cond::NE);
        assert!(incs.absorbed(0, 1), "add is emitted by the select");
        assert!(incs.absorbed(0, 0), "icmp is emitted by the select");
    }

    #[test]
    fn second_reader_of_the_add_blocks_the_fuse() {
        let f = count_func(true, false);
        let incs = fusible_select_incs(&f);
        assert!(incs.plan(0, 2).is_none());
        assert!(!incs.absorbed(0, 1));
    }

    #[test]
    fn second_reader_of_the_cond_keeps_add_fuse_but_drops_cmp() {
        let f = count_func(false, true);
        let incs = fusible_select_incs(&f);
        let plan = incs.plan(0, 2).expect("add still absorbs");
        assert!(plan.cmp.is_none(), "multi-use cond cannot ride NZCV");
        // falls back to `cmp cond, #0`; increment on the then arm
        // inverts NE into EQ.
        assert_eq!(plan.cc, cond::EQ);
        assert!(incs.absorbed(0, 1));
        assert!(!incs.absorbed(0, 0), "icmp must still emit its cset");
    }

    #[test]
    fn increment_on_the_else_arm_keeps_the_plain_predicate() {
        let mut f = count_func(false, false);
        // swap the arms: select %0, %n, %1
        f.blocks[0].insts[2] = mk(
            2,
            InstKind::Select(
                Type::I64,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(5),
                Operand::Value(ValueId(1)),
            ),
        );
        let incs = fusible_select_incs(&f);
        let plan = incs.plan(0, 2).expect("mirrored arms still recognized");
        assert_eq!(plan.cc, cond::EQ, "eq predicate passes through");
    }

    #[test]
    fn non_unit_addend_is_not_an_increment() {
        let mut f = count_func(false, false);
        f.blocks[0].insts[1] = mk(
            1,
            InstKind::BinOp(BinOp::Add, Operand::ConstI64(5), Operand::ConstI64(2)),
        );
        let incs = fusible_select_incs(&f);
        assert!(incs.plan(0, 2).is_none());
    }

    #[test]
    fn f64_select_never_takes_the_x_form() {
        let mut f = count_func(false, false);
        f.blocks[0].insts[2] = mk(
            2,
            InstKind::Select(
                Type::F64,
                Operand::Value(ValueId(0)),
                Operand::Value(ValueId(1)),
                Operand::ConstI64(5),
            ),
        );
        let incs = fusible_select_incs(&f);
        assert!(incs.plan(0, 2).is_none());
    }

    #[test]
    fn invert_cond_pairs_match_the_arm_arm_table() {
        assert_eq!(invert_cond(cond::EQ), cond::NE);
        assert_eq!(invert_cond(cond::NE), cond::EQ);
        assert_eq!(invert_cond(cond::GE), cond::LT);
        assert_eq!(invert_cond(cond::LT), cond::GE);
        assert_eq!(invert_cond(cond::GT), cond::LE);
        assert_eq!(invert_cond(cond::LE), cond::GT);
    }
}
