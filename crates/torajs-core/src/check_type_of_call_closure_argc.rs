//! RFC 20260708-closure-argc-abi chunk 1 — beyond-arity admit for a
//! length-only real-argc closure binding (`const f = function () {
//! return arguments.length; }; f(1, 2, 3)`).
//!
//! `desugar_arguments_object` recorded the safe bindings in
//! `ast.closure_argc_locals`. The body reads the S1 hidden-ABI
//! `__torajs_argc` (RFC 20260810-indirect-argc-abi S3.2 — the
//! injected `__torajs_real_argc` slot and its synthetic-param pop
//! retired in S3.4), so this wedge's one remaining job is admitting
//! beyond-arity calls: the extra args still typecheck individually
//! (ES §13.3.6.1 ArgumentListEvaluation) but drop out of the param
//! pairing — the length-only tier never reads their values, only
//! their count, which the call's hidden argc slot carries.
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
    params: &[Type],
    effective_args: &mut Vec<ExprId>,
) -> Result<(), String> {
    let Expr::Ident(name) = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !ast.closure_argc_locals.contains(name) {
        return Ok(());
    }
    if effective_args.len() > params.len() {
        for a in &effective_args[params.len()..] {
            checker.type_of(ast, *a)?;
        }
        effective_args.truncate(params.len());
    }
    Ok(())
}
