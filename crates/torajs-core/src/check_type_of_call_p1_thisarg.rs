//! P1 / S270 — Array.prototype callback methods trailing
//! thisArg drop wedge extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 297 — eighty-ninth sub-
//! batch of check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §23.1.3.X: `Array.prototype.{map,filter,every,
//! some,forEach,find,findIndex,findLast,findLastIndex,
//! flatMap}` each accept an optional trailing `thisArg`
//! after the callback (and S270 widens the drop to `>=
//! params+1` so any trailing args past thisArg are also
//! silent-dropped per the same ES §23.1.3.X trailing-arg
//! ignore — `xs.map(cb, thisArg, ...trailing)` is spec-legal).
//! tora's closures don't have `this` semantics so the
//! thisArg is silently dropped; we still `type_of` each
//! dropped arg so internal expression errors surface, but
//! SSA-emit reads only args[0] (cb).
//!
//! Unlike the dispatch-arm siblings (which return
//! `Option<Result<Type, String>>` and delegate the rest of
//! the cascade through a single early-return), this wedge
//! mutates `effective_args` in place and returns
//! `Result<(), String>` so the caller can keep flowing
//! into the rest of the generic-call typecheck. The mutator
//! shape mirrors what was inline before extraction; the
//! body is otherwise verbatim.
//!
//! Returns:
//! - `Ok(())` — wedge consumed (drop applied) or did not
//!   fire (non-Member callee, m_name outside the allowlist,
//!   or args.len() < params.len() + 1)
//! - `Err(_)` — any dropped trailing arg's `type_of` failed

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Checker;

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    params_len: usize,
    effective_args: &mut Vec<ExprId>,
) -> Result<(), String> {
    if effective_args.len() < params_len + 1 {
        return Ok(());
    }
    let Expr::Member { name: m_name, .. } = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !matches!(
        m_name.as_str(),
        "map"
            | "filter"
            | "every"
            | "some"
            | "forEach"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "flatMap"
    ) {
        return Ok(());
    }
    for &arg in &effective_args[params_len..] {
        let _ = checker.type_of(ast, arg)?;
    }
    effective_args.truncate(params_len);
    Ok(())
}
