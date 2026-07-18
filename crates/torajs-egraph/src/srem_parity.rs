//! Parity-compare strength reduction — a `srem x, 2^k` whose every
//! use is an eq/ne compare against 0 rewrites to `and x, 2^k-1`.
//!
//! Zero-ness is divisibility: `srem(x, 2^k) == 0  ⟺  2^k | x  ⟺
//! (x & (2^k-1)) == 0`, for every two's-complement x including
//! negatives (srem keeps the dividend's sign but a zero remainder is
//! sign-free). The non-zero remainders differ between the two forms,
//! so the rewrite demands that *all* uses are eq/ne-vs-0 compares —
//! any other consumer (arithmetic, ret, branch cond, ordered compare)
//! keeps the srem.
//!
//! Why this matters: codegen expands SRem into `sdiv + msub`
//! (compile/binop.rs), and the sdiv hidden inside that expansion is
//! invisible to SSA-level GVN — a parity test in a division loop pays
//! a full divider trip (~8 cyc + divider occupancy) for one AND worth
//! of information. float_demote's FRem→SRem output is the main feeder
//! (collatz decomposition A1b,
//! .claude/tasks/2026-07-18/perf-collatz-decomposition.md).
//!
//! Runs after float_demote (its SRem output is the target shape),
//! before sext_elide / ctpop_idiom. `TORAJS_SREM_PARITY_OFF=1` skips,
//! `TORAJS_SREM_PARITY_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, IPred, InstKind, Module, Operand, Terminator, ValueId};

use crate::interval::transfer::op_const_int;
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SremParityStats {
    /// srem-by-2^k defs rewritten to and-by-mask.
    pub srems_reduced: u32,
}

pub fn reduce_parity_srems(module: &mut Module) -> SremParityStats {
    let mut stats = SremParityStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        reduce_in_function(func, &mut stats);
    }
    stats
}

/// One eq/ne compare against constant 0, with `v` on the value side.
fn is_zero_compare_of(kind: &InstKind, v: ValueId) -> bool {
    let InstKind::ICmp(IPred::Eq | IPred::Ne, a, b) = kind else {
        return false;
    };
    match (a, b) {
        (Operand::Value(x), c) | (c, Operand::Value(x)) if *x == v => op_const_int(c) == Some(0),
        _ => false,
    }
}

fn reduce_in_function(func: &mut Function, stats: &mut SremParityStats) {
    // candidate defs: %r = srem x, 2^k (k ≥ 1)
    let mut candidates: HashMap<ValueId, (usize, usize)> = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let (Some(r), InstKind::BinOp(BinOp::SRem, _, rhs)) = (&inst.result, &inst.kind)
                && let Some(c) = op_const_int(rhs)
                && c >= 2
                && (c as u64).is_power_of_two()
            {
                candidates.insert(*r, (bi, ii));
            }
        }
    }
    if candidates.is_empty() {
        return;
    }
    // disqualify every candidate with a use that is not an
    // eq/ne-vs-0 compare (terminator cond / ret uses disqualify too)
    let mut disqualified: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if candidates.contains_key(&v) && !is_zero_compare_of(&inst.kind, v) {
                    disqualified.insert(v);
                }
            });
        }
        let mut term_use = |op: &Operand| {
            if let Operand::Value(v) = op
                && candidates.contains_key(v)
            {
                disqualified.insert(*v);
            }
        };
        match &block.term {
            Terminator::Ret(Some(op)) => term_use(op),
            Terminator::CondBr { cond, .. } => term_use(cond),
            _ => {}
        }
    }
    for (r, (bi, ii)) in candidates {
        if disqualified.contains(&r) {
            continue;
        }
        let inst = &mut func.blocks[bi].insts[ii];
        let InstKind::BinOp(BinOp::SRem, x, rhs) = &inst.kind else {
            unreachable!("candidate site holds the srem");
        };
        let mask = (op_const_int(rhs).expect("candidate divisor is const") - 1) as i64;
        inst.kind = InstKind::BinOp(BinOp::And, *x, Operand::ConstI64(mask));
        stats.srems_reduced += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

    fn val(result: u32, kind: InstKind, values: &mut Vec<ValueInfo>) -> Inst {
        assert_eq!(values.len(), result as usize);
        values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        Inst {
            result: Some(ValueId(result)),
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

    fn func_one_block(insts: Vec<Inst>, values: Vec<ValueInfo>, term: Terminator) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term,
            }],
            values,
            current_origin: None,
        }
    }

    #[test]
    fn parity_srem_reduces_to_and() {
        let mut vals = vec![];
        let insts = vec![
            val(0, InstKind::BinOp(BinOp::Add, c(5), c(2)), &mut vals),
            val(1, InstKind::BinOp(BinOp::SRem, v(0), c(2)), &mut vals),
            val(2, InstKind::ICmp(IPred::Eq, v(1), c(0)), &mut vals),
        ];
        let mut f = func_one_block(insts, vals, Terminator::Ret(Some(v(2))));
        let mut stats = SremParityStats::default();
        reduce_in_function(&mut f, &mut stats);
        assert_eq!(stats.srems_reduced, 1);
        assert_eq!(
            f.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::And, v(0), c(1))
        );
    }

    #[test]
    fn pow2_masks_larger_divisors() {
        // srem x, 8 compared ne 0 (0 on the lhs) → and x, 7
        let mut vals = vec![];
        let insts = vec![
            val(0, InstKind::BinOp(BinOp::Add, c(5), c(2)), &mut vals),
            val(1, InstKind::BinOp(BinOp::SRem, v(0), c(8)), &mut vals),
            val(2, InstKind::ICmp(IPred::Ne, c(0), v(1)), &mut vals),
        ];
        let mut f = func_one_block(insts, vals, Terminator::Ret(Some(v(2))));
        let mut stats = SremParityStats::default();
        reduce_in_function(&mut f, &mut stats);
        assert_eq!(stats.srems_reduced, 1);
        assert_eq!(
            f.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::And, v(0), c(7))
        );
    }

    #[test]
    fn non_compare_use_keeps_srem() {
        // the remainder also feeds an add — value matters, no rewrite
        let mut vals = vec![];
        let insts = vec![
            val(0, InstKind::BinOp(BinOp::Add, c(5), c(2)), &mut vals),
            val(1, InstKind::BinOp(BinOp::SRem, v(0), c(2)), &mut vals),
            val(2, InstKind::ICmp(IPred::Eq, v(1), c(0)), &mut vals),
            val(3, InstKind::BinOp(BinOp::Add, v(1), v(0)), &mut vals),
        ];
        let mut f = func_one_block(insts, vals, Terminator::Ret(Some(v(3))));
        let mut stats = SremParityStats::default();
        reduce_in_function(&mut f, &mut stats);
        assert_eq!(stats.srems_reduced, 0);
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::SRem, _, _)
        ));
    }

    #[test]
    fn non_pow2_and_ordered_compare_keep_srem() {
        // srem x, 3 — not a power of two
        let mut vals = vec![];
        let insts = vec![
            val(0, InstKind::BinOp(BinOp::Add, c(5), c(2)), &mut vals),
            val(1, InstKind::BinOp(BinOp::SRem, v(0), c(3)), &mut vals),
            val(2, InstKind::ICmp(IPred::Eq, v(1), c(0)), &mut vals),
        ];
        let mut f = func_one_block(insts, vals, Terminator::Ret(Some(v(2))));
        // srem x, 2 under an ordered compare — sign observable
        let mut vals_g = vec![];
        let insts_g = vec![
            val(0, InstKind::BinOp(BinOp::Add, c(5), c(2)), &mut vals_g),
            val(1, InstKind::BinOp(BinOp::SRem, v(0), c(2)), &mut vals_g),
            val(2, InstKind::ICmp(IPred::Slt, v(1), c(0)), &mut vals_g),
        ];
        let mut g = func_one_block(insts_g, vals_g, Terminator::Ret(Some(v(2))));
        let mut stats = SremParityStats::default();
        reduce_in_function(&mut f, &mut stats);
        reduce_in_function(&mut g, &mut stats);
        assert_eq!(stats.srems_reduced, 0);
    }
}
