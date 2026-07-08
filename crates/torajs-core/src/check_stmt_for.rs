//! `Stmt::For { init, cond, step, body }` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::For` arm as
//! chunk-103 of the check_stmt decomp. 6th check_stmt sibling.
//!
//! Init runs in a fresh scope so `let i = 0; ...; for () i;`
//! doesn't bleed `i` into the surrounding fn scope. Push a scope
//! before init, pop after body.
//!
//! Step:
//! 1. Push fresh scope.
//! 2. If init present — check_stmt it (allows let-decl + assign).
//! 3. If cond present — typecheck as truthy-acceptable.
//! 4. If step present — typecheck for side-effect well-formedness.
//! 5. check_stmt body.
//! 6. Pop scope.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, DiagPush, js_truthy_acceptable};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    init: &Option<Box<Stmt>>,
    cond: &Option<ExprId>,
    step: &Option<ExprId>,
    body: &Stmt,
) {
    checker.scopes.push(HashMap::new());
    if let Some(i) = init {
        checker.check_stmt(ast, i);
    }
    // ut3 — init runs once but cond/step/body sit on the loop
    // back-edge: an init assignment narrow must not reach them.
    checker.flush_assign_narrows();
    if let Some(c) = cond {
        match checker.type_of(ast, *c) {
            Ok(t) if js_truthy_acceptable(&t) => {}
            Ok(other) => checker.errors.push_err(format!(
                "for condition must be boolean (or coercible), got {other:?}"
            )),
            Err(e) => checker.errors.push_err(e),
        }
    }
    if let Some(st) = step {
        if let Err(e) = checker.type_of(ast, *st) {
            checker.errors.push_err(e);
        }
    }
    checker.check_stmt(ast, body);
    checker.scopes.pop();
}
