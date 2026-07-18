//! Per-def-site demotion judgments: how a def joins the set (exact /
//! guarded / not), the -0-free proof for bridged escapes, and the
//! i64 rewrite forms. Pure predicates over the interval facts — no
//! IR is mutated here.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, FPred, IPred, InstKind, Operand, Type, ValueId};

use crate::interval::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};

use super::guard;

const F64_EXACT: i128 = 1i128 << 53;

/// A fact whose every producing op was exact in f64 (clamp53 would
/// have widened a rounding bound to a sentinel).
fn exact(f: &NumFact) -> bool {
    f.lo > -F64_EXACT && f.hi < F64_EXACT && f.lo > NEG_INF && f.hi < POS_INF
}

pub(crate) fn fpred_to_ipred(p: FPred) -> IPred {
    match p {
        FPred::Oeq => IPred::Eq,
        FPred::One | FPred::Une => IPred::Ne,
        FPred::Olt => IPred::Slt,
        FPred::Ogt => IPred::Sgt,
        FPred::Ole => IPred::Sle,
        FPred::Oge => IPred::Sge,
    }
}

/// Operand qualifies as an in-set value or an integer constant.
fn op_ok(op: &Operand, set: &HashSet<ValueId>) -> bool {
    match op {
        Operand::Value(v) => set.contains(v),
        _ => op_const_int(op).is_some(),
    }
}

/// An fcmp convertible to icmp: at least one demoted operand, every
/// operand demoted or an integer constant.
pub(crate) fn fcmp_convertible(a: &Operand, b: &Operand, set: &HashSet<ValueId>) -> bool {
    let in_set = |op: &Operand| matches!(op, Operand::Value(v) if set.contains(v));
    (in_set(a) || in_set(b)) && op_ok(a, set) && op_ok(b, set)
}

/// How one def site can join the demotion set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    /// Provably exact — rewrites with no runtime check.
    Exact,
    /// Exact only under a runtime guard (1b-ii).
    Guarded,
    /// Not demotable.
    No,
}

pub(crate) fn def_fit(
    d: &InstKind,
    v: &ValueId,
    facts: &HashMap<ValueId, NumFact>,
    set: &HashSet<ValueId>,
) -> Fit {
    if def_exact(d, v, facts, set) {
        return Fit::Exact;
    }
    match d {
        InstKind::SiToFp(x @ Operand::Value(_)) => match guard::entry_checks(x, facts) {
            Some(checks) if !checks.is_empty() => Fit::Guarded,
            _ => Fit::No,
        },
        InstKind::BinOp(BinOp::FMul | BinOp::FAdd | BinOp::FSub, a, b) => {
            let vars_in_set = match (a, b) {
                (Operand::Value(av), other) if op_const_int(other).is_some() => set.contains(av),
                (other, Operand::Value(bv)) if op_const_int(other).is_some() => set.contains(bv),
                (Operand::Value(av), Operand::Value(bv)) => set.contains(av) && set.contains(bv),
                _ => false,
            };
            if vars_in_set && guard::growth_checks(d, *v, facts).is_some() {
                Fit::Guarded
            } else {
                Fit::No
            }
        }
        _ => Fit::No,
    }
}

/// One def site is rewritable to an exact i64 equivalent with no
/// runtime check.
pub(crate) fn def_exact(
    d: &InstKind,
    v: &ValueId,
    facts: &HashMap<ValueId, NumFact>,
    set: &HashSet<ValueId>,
) -> bool {
    match d {
        InstKind::SiToFp(x) => match x {
            Operand::Value(xv) => facts.get(xv).is_some_and(exact),
            _ => op_const_int(x).is_some(),
        },
        InstKind::BinOp(BinOp::FRem, a, b) => {
            op_ok(a, set)
                && (op_const_int(b).is_some_and(|c| c != 0)
                    // W3 C3 — variable divisor: the interval transfer
                    // assigns a var-divisor frem a fact only under a
                    // dominating ≠0 refinement (`while (b !== 0)`),
                    // so the fact's presence is the nonzero-divisor
                    // certificate (FDiv evenness channel, below).
                    || (op_ok(b, set) && facts.contains_key(v)))
        }
        // the interval transfer only assigns an fdiv result a fact
        // under the frem-parity guard — its presence IS the evenness
        // certificate; exactness needs no bound check (halving).
        InstKind::BinOp(BinOp::FDiv, a, b) => {
            op_ok(a, set) && op_const_int(b) == Some(2) && facts.contains_key(v)
        }
        InstKind::BinOp(BinOp::FAdd | BinOp::FSub | BinOp::FMul, a, b) => {
            op_ok(a, set) && op_ok(b, set) && facts.get(v).is_some_and(exact)
        }
        InstKind::Copy(Type::F64, a) | InstKind::Identity(a) => op_ok(a, set),
        _ => false,
    }
}

/// One def site provably never produces -0.0 given `nz` operands.
pub(crate) fn def_no_neg_zero(
    d: &InstKind,
    facts: &HashMap<ValueId, NumFact>,
    nz: &HashSet<ValueId>,
) -> bool {
    let op_nz = |op: &Operand| match op {
        Operand::Value(v) => nz.contains(v),
        Operand::ConstF64(f) => !(*f == 0.0 && f.is_sign_negative()),
        _ => op_const_int(op).is_some(),
    };
    let nonneg = |op: &Operand| match op {
        Operand::Value(v) => facts.get(v).is_some_and(|f| f.lo >= 0),
        _ => op_const_int(op).is_some_and(|c| c >= 0),
    };
    match d {
        // integer-to-float conversion never yields -0
        InstKind::SiToFp(_) => true,
        // fmod keeps the dividend's sign: non-negative -0-free
        // dividend ⇒ result in [+0, C)
        InstKind::BinOp(BinOp::FRem, a, _) => op_nz(a) && nonneg(a),
        // halving a -0-free value: only ±0 halves to ±0, and the
        // input zero is +0
        InstKind::BinOp(BinOp::FDiv, a, _) => op_nz(a),
        // x + y / x − y yields -0 only when both inputs are -0-shaped
        InstKind::BinOp(BinOp::FAdd | BinOp::FSub, a, b) => op_nz(a) && op_nz(b),
        // a*b yields -0 from a ±0 with a negative cofactor: demand
        // sign-clean non-negative -0-free operands
        InstKind::BinOp(BinOp::FMul, a, b) => op_nz(a) && op_nz(b) && nonneg(a) && nonneg(b),
        InstKind::Copy(_, a) | InstKind::Identity(a) => op_nz(a),
        _ => false,
    }
}

/// The i64 form of a demoted def site.
pub(crate) fn rewrite_def(d: &InstKind) -> InstKind {
    match d {
        InstKind::SiToFp(x) => InstKind::Copy(Type::I64, int_op(x)),
        InstKind::BinOp(BinOp::FRem, a, b) => InstKind::BinOp(BinOp::SRem, int_op(a), int_op(b)),
        // fits_int only admits FDiv under the by-2 evenness
        // certificate, where an arithmetic shift halves exactly
        // (negative evens included; sdiv-by-2 and asr-1 disagree only
        // on negative odds, excluded by the certificate) — sdiv is
        // ~8x the latency of asr on the carried chain
        InstKind::BinOp(BinOp::FDiv, a, b) if op_const_int(b) == Some(2) => {
            InstKind::BinOp(BinOp::AShr, int_op(a), Operand::ConstI64(1))
        }
        InstKind::BinOp(BinOp::FDiv, a, b) => InstKind::BinOp(BinOp::SDiv, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FAdd, a, b) => InstKind::BinOp(BinOp::Add, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FSub, a, b) => InstKind::BinOp(BinOp::Sub, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FMul, a, b) => InstKind::BinOp(BinOp::Mul, int_op(a), int_op(b)),
        // int_op matters on the const arms: a GVN-folded integer
        // ConstF64 operand must become ConstI64 (the baseline tier's
        // GPR materialization rejects f64 constants)
        InstKind::Copy(Type::F64, a) => InstKind::Copy(Type::I64, int_op(a)),
        InstKind::Identity(a) => InstKind::Identity(int_op(a)),
        other => other.clone(),
    }
}

/// Integer-const form of an operand (values pass through unchanged).
pub(crate) fn int_op(op: &Operand) -> Operand {
    match op {
        Operand::Value(_) => *op,
        other => match op_const_int(other) {
            Some(c) => Operand::ConstI64(c as i64),
            None => *other,
        },
    }
}
