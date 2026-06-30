//! Array / String `at()` arity narrow wedge extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 302 — ninety-fourth
//! sub-batch of check_type_of_call.rs per-shape
//! decomposition).
//!
//! Per ES §22.1.3.1 / §23.1.3.1 step 2-3:
//! `undefined` routes through ToIntegerOrInfinity → 0, so
//! the 0-arg form `arr.at()` / `s.at()` returns index 0.
//! The declared `Type::Function` sig is the 1-arg full
//! form; truncate to 0 params on the no-arg call so the
//! strict-equality arity check that follows accepts it.
//! `ssa_lower_str` Arr and String `at` emitters fill the
//! default `ConstI64(0)` at emit time.
//!
//! Mutator-shape sibling (mirrors chunk 299 Date setter /
//! chunk 300 splice narrow): returns `Result<(), String>`,
//! borrows `effective_args` read-only because the wedge
//! only truncates `params`.
//!
//! Returns:
//! - `Ok(())` — wedge applied (params truncated) or did
//!   not fire (effective_args.len() >= params.len(),
//!   callee not a Member, m_name != "at", receiver not
//!   Array / String)
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
    if name != "at" {
        return Ok(());
    }
    let recv_ty = checker.type_of(ast, *obj)?;
    if matches!(recv_ty, Type::Array(_) | Type::String) {
        params.truncate(effective_args.len());
    }
    Ok(())
}
