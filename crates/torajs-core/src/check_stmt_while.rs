//! `Stmt::While { cond, body }` + `Stmt::DoWhile { body, cond }`
//! typecheck pulled out of [`crate::check::Checker::check_stmt`]'s
//! corresponding arms as chunk-102 of the check_stmt decomp. 5th
//! check_stmt sibling.
//!
//! - **While** — V3-18 narrow wedge on `(x !== null)` conditions:
//!   narrow `x` for the body ONLY if the body doesn't reassign `x`
//!   (otherwise the re-narrowing would conflict with the
//!   still-Nullable RHS of `x = x.next`). Polarity-false
//!   (`x === null`) wouldn't enter the loop, so no narrow.
//! - **DoWhile** — body runs first (no narrow); condition typechecks
//!   after.

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, js_truthy_acceptable, stmt_assigns_to};

pub(crate) fn check_while(checker: &mut Checker, ast: &Ast, cond: ExprId, body: &Stmt) {
    match checker.type_of(ast, cond) {
        Ok(t) if js_truthy_acceptable(&t) => {}
        Ok(other) => checker.errors.push_err(format!(
            "while condition must be boolean (or coercible), got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
    let narrow = checker.collect_null_narrow(ast, cond);
    let saved = if let Some((name, inner, polarity)) = &narrow {
        if *polarity && !stmt_assigns_to(ast, body, name) {
            checker.apply_narrow(name, inner.clone())
        } else {
            None
        }
    } else {
        None
    };
    checker.check_stmt(ast, body);
    if let (Some((name, _, _)), Some(prev)) = (&narrow, saved) {
        checker.restore_narrow(name, prev);
    }
}

pub(crate) fn check_do_while(checker: &mut Checker, ast: &Ast, body: &Stmt, cond: ExprId) {
    checker.check_stmt(ast, body);
    match checker.type_of(ast, cond) {
        Ok(t) if js_truthy_acceptable(&t) => {}
        Ok(other) => checker.errors.push_err(format!(
            "do-while condition must be boolean (or coercible), got {other:?}"
        )),
        Err(e) => checker.errors.push_err(e),
    }
}
