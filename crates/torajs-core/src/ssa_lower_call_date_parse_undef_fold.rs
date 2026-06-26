//! S230 — `Date.parse(undefined)` static-fold pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-56 of the decomp (chunks 1-55 = ... + JSON.stringify
//! trampoline).
//!
//! ES §21.4.3.2 step 1 reads the arg through `ToString`.
//! `ToString(undefined) = "undefined"`, which is not a valid date
//! string, so the result is NaN. Fold the call before `lower_expr`
//! would emit a `ConstPtrNull` undef into the helper's `(Str) → F64`
//! ABI.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Date.parse` Member-Ident shape, or args arity != 1, or args[0]
//! not statically typed `check::Type::Undefined`). All other shapes
//! fall through to the generic resolve_callee path.

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 1
        || !matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "parse" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Date" {
        return None;
    }
    Some(Operand::ConstF64(f64::NAN))
}
