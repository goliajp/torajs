//! Strict equality between two F64 operands when either can wear the
//! `undefined` sentinel.
//!
//! `ssa_lower_binop_null_undef` already handles the shape where one
//! side is the `undefined` LITERAL. This is the other one: both sides
//! are values, and at least one of them came from a slot the sentinel
//! analysis armed. Nothing compared them as the values they stand
//! for, so the plain `FCmp` saw two quiet NaNs and answered by IEEE
//! rules — `u !== u` was true, which is the canonical JS spelling of
//! "is this NaN" and the exact question undefined must answer no to.
//!
//! §7.2.16 SameValueNonNumber makes undefined equal only to itself,
//! so the three cases are: both sentinels → equal; exactly one →
//! not equal; neither → the FCmp that was always emitted.
//!
//! A side the analysis did NOT arm cannot hold the sentinel —
//! FPCR.DN means the machine has no way to forge the payload — so
//! its test folds to a constant here rather than costing an
//! instruction.

use crate::ssa::{BinOp as SsaBinOp, FPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `a === b` for two F64 operands, honouring the sentinel. `neq`
/// asks for the negation.
pub(crate) fn strict_eq_f64(
    ctx: &mut LowerCtx<'_>,
    a: Operand,
    b: Operand,
    a_armed: bool,
    b_armed: bool,
    neq: bool,
) -> Operand {
    let numeric = ctx.fcmp(FPred::Oeq, a.clone(), b.clone());
    let eq = eq_with_sentinel(ctx, numeric, a, b, a_armed, b_armed);
    if neq { bool_not(ctx, eq) } else { eq }
}

/// Fold an equality that was decided on the numbers alone into one
/// that knows about the sentinel: both sides undefined is equal,
/// exactly one is not, neither defers to `numeric`.
///
/// `Object.is` shares this and not [`strict_eq_f64`], because
/// SameValue and strict equality disagree about NaN and about signed
/// zero — only the undefined half is common, and it is this.
pub(crate) fn eq_with_sentinel(
    ctx: &mut LowerCtx<'_>,
    numeric: Operand,
    a: Operand,
    b: Operand,
    a_armed: bool,
    b_armed: bool,
) -> Operand {
    let a_undef = if a_armed {
        ctx.f64_is_sentinel(a)
    } else {
        Operand::ConstBool(false)
    };
    let b_undef = if b_armed {
        ctx.f64_is_sentinel(b)
    } else {
        Operand::ConstBool(false)
    };
    let both = bool_op(ctx, SsaBinOp::And, a_undef.clone(), b_undef.clone());
    let either = bool_op(ctx, SsaBinOp::Or, a_undef, b_undef);
    let neither = bool_not(ctx, either);
    let numeric_eq = bool_op(ctx, SsaBinOp::And, neither, numeric);
    bool_op(ctx, SsaBinOp::Or, both, numeric_eq)
}

fn bool_op(ctx: &mut LowerCtx<'_>, op: SsaBinOp, a: Operand, b: Operand) -> Operand {
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::BinOp(op, a, b), Type::Bool, None);
    Operand::Value(v)
}

fn bool_not(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    bool_op(ctx, SsaBinOp::Xor, v, Operand::ConstBool(true))
}
