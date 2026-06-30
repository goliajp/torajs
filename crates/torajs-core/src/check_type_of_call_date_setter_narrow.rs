//! Date per-field setter arity narrow wedge extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 299 — ninety-first sub-batch
//! of check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §21.4.4.20-26: `Date.prototype.{setFullYear,
//! setMonth,setHours,setMinutes,setSeconds}` each accept
//! 1-N args with trailing positions optional. The declared
//! `Type::Function` sig is the full (max-arity) form; when
//! the caller supplied fewer args we narrow `params` to the
//! supplied arity so the strict-equality arity check that
//! follows passes. ssa_lower sentinel-pads the missing
//! trailing positions with `DATE_FIELD_KEEP` (i64::MIN).
//!
//! Mutator-shape sibling (mirrors chunk 297's P1 wedge):
//! returns `Result<(), String>` so the caller keeps flowing
//! into the subsequent splice / at wedges and the strict-
//! arity check.
//!
//! Returns:
//! - `Ok(())` — wedge applied (params truncated) or did not
//!   fire (effective_args.len() >= params.len(), callee not
//!   a Member, m_name outside the 5-setter allowlist,
//!   receiver not Type::Date, or effective_args.len() == 0)
//! - `Err(_)` — receiver `type_of` failed

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &[ExprId],
    params: &mut Vec<Type>,
) -> Result<(), String> {
    if effective_args.len() >= params.len() {
        return Ok(());
    }
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !matches!(
        name.as_str(),
        "setFullYear" | "setMonth" | "setHours" | "setMinutes" | "setSeconds"
    ) {
        return Ok(());
    }
    let recv_ty = checker.type_of(ast, *obj)?;
    if recv_ty == Type::Date && !effective_args.is_empty() {
        params.truncate(effective_args.len());
    }
    Ok(())
}
