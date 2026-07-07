//! RFC 20260708-closure-argc-abi chunk 1 — call wedge for a
//! length-only real-argc closure binding (`const f = function () {
//! return arguments.length; }; f(1, 2, 3)`).
//!
//! `desugar_arguments_object` injected `__torajs_real_argc: number`
//! as the closure's first USER param and recorded the safe bindings
//! in `ast.closure_argc_locals`, so the binding's checker type is
//! `Function([number, ...declared], ret)` while the user call never
//! supplies the argc slot (ssa_lower's closure-local arm prepends it
//! at runtime). This wedge:
//!
//! 1. pops the synthetic argc slot off `params`, and
//! 2. admits beyond-arity calls — the extra args still typecheck
//!    individually (ES §13.3.6.1 ArgumentListEvaluation) but drop
//!    out of the param pairing; the length-only tier never reads
//!    their values, only their count.
//!
//! Fewer-than-declared calls fall through untouched: the remaining
//! declared params are all Any (P0.5 unannotated-closure default),
//! so the T-28 pad wedge right after this one records the
//! `arity_pad_count` the same way it does for any closure call.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    params: &mut Vec<Type>,
    effective_args: &mut Vec<ExprId>,
) -> Result<(), String> {
    let Expr::Ident(name) = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !ast.closure_argc_locals.contains(name) || params.is_empty() {
        return Ok(());
    }
    params.remove(0);
    if effective_args.len() > params.len() {
        for a in &effective_args[params.len()..] {
            checker.type_of(ast, *a)?;
        }
        effective_args.truncate(params.len());
    }
    Ok(())
}
