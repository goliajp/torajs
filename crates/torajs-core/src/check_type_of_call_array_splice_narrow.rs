//! Array splice / toSpliced arity narrow wedge extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 300 — ninety-second sub-batch
//! of check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §23.1.3.31: `Array.prototype.splice(start,
//! deleteCount?, ...items)` and the §23.1.3.34 immutable
//! sibling `toSpliced` accept 0 / 1 / 2+ arg forms with
//! `start = 0` / `deleteCount = len - actualStart` as the
//! spec defaults. The declared `Type::Function` sig is the
//! full 2+-arg form; when the caller supplied fewer args we
//! narrow `params` to the supplied arity so the strict-
//! equality arity check that follows passes. `ssa_lower_splice`
//! siblings fill the missing trailing positions at emit time
//! (0 → `ConstI64(0)`, 1 → `i64::MAX` sentinel that the
//! helper clamps to `len - start`).
//!
//! Mutator-shape sibling (mirrors chunk 299's Date setter):
//! returns `Result<(), String>` so the caller keeps flowing
//! into the subsequent splice-undef / at wedges and the
//! strict-arity check.
//!
//! Returns:
//! - `Ok(())` — wedge applied (params truncated) or did not
//!   fire (effective_args.len() >= params.len(), callee not
//!   a Member, m_name outside the splice/toSpliced allowlist,
//!   receiver not Type::Array)
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
    if !matches!(name.as_str(), "splice" | "toSpliced") {
        return Ok(());
    }
    let recv_ty = checker.type_of(ast, *obj)?;
    if matches!(recv_ty, Type::Array(_)) {
        params.truncate(effective_args.len());
    }
    Ok(())
}
