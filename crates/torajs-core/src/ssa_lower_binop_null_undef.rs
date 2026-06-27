//! ES §7.2.15 `null === undefined` / `undefined === null` static
//! fold pulled out of [`crate::ssa_lower::lower_binop_inner`] as
//! chunk-87 of the decomp.
//!
//! Both `null` and `undefined` literals lower to `ConstPtrNull` at
//! the SSA layer, so the downstream same-family ptr-cmp would
//! return true for `null === undefined`. Spec §6.1.1 / §6.1.2
//! treat them as distinct primitive types — `===` must be false,
//! `!==` must be true.
//!
//! Distinguish via the `binop_{left,right}_undef_id` flag set in
//! `lower_binop_with_ids` when the source-level expr type was
//! `Type::Undefined`. If exactly one side is Undefined-typed and
//! the other is the bare `null` literal, static-fold to ConstBool
//! before the downstream same-family cmp path can see it.
//!
//! Returns `Some(ConstBool)` on a match; falls through (`None`)
//! otherwise.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        return None;
    }
    let a_is_null_lit = matches!(a, Operand::ConstPtrNull);
    let b_is_null_lit = matches!(b, Operand::ConstPtrNull);
    let a_is_undef = ctx.binop_left_undef_id.is_some();
    let b_is_undef = ctx.binop_right_undef_id.is_some();
    if (a_is_undef && b_is_null_lit && !a_is_null_lit)
        || (b_is_undef && a_is_null_lit && !b_is_null_lit)
    {
        let answer = matches!(op, AstBinOp::Neq);
        return Some(Operand::ConstBool(answer));
    }
    None
}
