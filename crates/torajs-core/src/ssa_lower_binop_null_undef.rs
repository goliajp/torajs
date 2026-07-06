//! ES §7.2.15 nullish strict-equality static folds pulled out of
//! [`crate::ssa_lower::lower_binop_inner`] as chunk-87 of the
//! decomp; rewritten type-based in chunk 612.
//!
//! Both `null` and `undefined` lower to a zero pointer at the SSA
//! layer — a raw ptr-cmp can't tell them apart, and an
//! Undefined/Null-typed BINDING is a Load (not `ConstPtrNull`), so
//! operand-shape checks can't either. Fold on the checker types
//! instead, via the `binop_{left,right}_{undef,null}_id` flags set
//! in `lower_binop_with_ids`:
//!
//! - Undefined vs Null (either order, literal or binding) → `===`
//!   false / `!==` true (spec §6.1.1/§6.1.2 distinct types).
//! - Undefined vs Undefined / Null vs Null → `===` true (both are
//!   single-value types).
//! - Anything else (one side not statically nullish) → fall
//!   through (`None`) to the Any lane / same-family cmp paths.
//!
//! The pre-612 operand-shape version mistook the `undefined`
//! LITERAL (also ConstPtrNull) for `null`, folding
//! `y === undefined` to false for an Undefined-typed `y`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &LowerCtx<'_>,
    op: AstBinOp,
    _a: Operand,
    _b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        return None;
    }
    let a_undef = ctx.binop_left_undef_id.is_some();
    let b_undef = ctx.binop_right_undef_id.is_some();
    let a_null = ctx.binop_left_null_id.is_some();
    let b_null = ctx.binop_right_null_id.is_some();
    if !((a_undef || a_null) && (b_undef || b_null)) {
        return None;
    }
    let eq = (a_undef && b_undef) || (a_null && b_null);
    let answer = match op {
        AstBinOp::Eq => eq,
        AstBinOp::Neq => !eq,
        _ => unreachable!(),
    };
    Some(Operand::ConstBool(answer))
}
