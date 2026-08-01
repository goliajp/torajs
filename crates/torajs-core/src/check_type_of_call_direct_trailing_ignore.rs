//! Direct-call trailing-arg ignore wedge — the general-tail
//! mirror of [`crate::check_type_of_call_math_date_trailing_ignore`]
//! for a bare-`Ident` callee (named fn / closure binding / fn-typed
//! param) or an inline `Expr::Closure` callee (an IIFE — the lift
//! pass replaced the fn-expr arena slot with its Closure repr).
//!
//! Per ES §13.3.6.1 ArgumentListEvaluation every supplied argument
//! evaluates in order; §10.2.1.3 OrdinaryCallBindThis + FunctionDeclarationInstantiation
//! then binds only the declared formals — calling any ECMAScript
//! function with more arguments than it declares is legal and the
//! extras are simply never read (test262 leans on this everywhere:
//! `(function fun() { … }(1, 2, 3))`).
//!
//! The wedge admits the beyond-arity call: the trailing operands
//! still `type_of` (their evaluation is real — side effects run at
//! the lowered call site), then drop out of the param pairing.
//! Every direct-call lowering arm aligns argv to the callee's
//! signature on its own (`ssa_lower_call_terminal` truncates,
//! `closure_local` / `fn_indirect` skip past the declared list), so
//! nothing extra crosses the ABI.
//!
//! Member-callee receivers (`Number.*` / `Array.prototype.*` …)
//! stay on their per-family carve-outs (S238–S248 / the Math-Date
//! wedge) — their lowering arms guard on exact arg shapes, and this
//! wedge widening them would strand calls between a declined
//! specialized arm and the generic terminal.
//!
//! Returns:
//! - `Ok(())` — wedge applied (effective_args truncated to
//!   params.len()) or did not fire (not over-arity, callee not a
//!   bare Ident)
//! - `Err(_)` — `type_of` failed on a trailing operand

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Checker;
use crate::check::Type;

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &mut Vec<ExprId>,
    params: &[Type],
) -> Result<(), String> {
    if effective_args.len() <= params.len() {
        return Ok(());
    }
    if !matches!(ast.get_expr(*callee), Expr::Ident(_) | Expr::Closure { .. }) {
        return Ok(());
    }
    for &aid in &effective_args[params.len()..] {
        let _ = checker.type_of(ast, aid)?;
    }
    effective_args.truncate(params.len());
    Ok(())
}
