//! The loop and `switch` arms of the await rewrite: a head that is
//! evaluated again on every iteration (or a case value evaluated only
//! while no earlier case matched) cannot take its awaits ahead of the
//! statement.

use super::super::desugar_async_sm::{expr_has_await, stmt_has_await};
use super::super::{Ast, Expr, ExprId, Stmt, UnaryOp};
use super::{Cx, lift_root, rewrite_list, rewrite_stmt, with_prefix};
use crate::ast::desugar_async_sm_lift::LiftLog;

/// The head-tested loop body: `{ <prefix>; if (!cond) break; body }`.
fn head_tested_body(ast: &mut Ast, mut prefix: Vec<Stmt>, cond: ExprId, body: Stmt) -> Stmt {
    let not = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: cond,
    });
    prefix.push(Stmt::If {
        cond: not,
        then_branch: Box::new(Stmt::Break(None)),
        else_branch: None,
    });
    prefix.push(body);
    Stmt::Block(prefix)
}

/// `for (init; c; step)` — the `while` head form for `c`; an await in
/// `init` or `step` keeps the refusal (init sits in the loop's own
/// scope, step runs after `continue`).
pub(super) fn rewrite_for(
    ast: &mut Ast,
    init: Option<&Stmt>,
    cond: &mut Option<ExprId>,
    step: Option<ExprId>,
    body: &mut Box<Stmt>,
    cx: &Cx,
    log: &mut LiftLog,
) -> bool {
    if init.is_some_and(|i| stmt_has_await(ast, i)) || step.is_some_and(|e| expr_has_await(ast, e))
    {
        return false;
    }
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let Some(c) = *cond else {
        return true;
    };
    let Some(prefix) = lift_root(ast, c, cx, log) else {
        return false;
    };
    if !prefix.is_empty() {
        let old = std::mem::replace(body.as_mut(), Stmt::Multi(Vec::new()));
        **body = head_tested_body(ast, prefix, c, old);
        *cond = None;
    }
    true
}

/// A loop head re-evaluates per iteration, so its await cannot hoist
/// ahead of the loop: `while (c)` becomes `while (true) { <lift of c>;
/// if (!c') break; body }` — `continue` in the body returns to the head
/// and re-runs the lift.
pub(super) fn rewrite_while(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::While { cond, body } = s else {
        unreachable!("rewrite_while on a while")
    };
    if !rewrite_stmt(ast, body, cx, log) {
        return false;
    }
    let cond = *cond;
    let Some(prefix) = lift_root(ast, cond, cx, log) else {
        return false;
    };
    if !prefix.is_empty() {
        let body = std::mem::replace(body.as_mut(), Stmt::Multi(Vec::new()));
        let new_body = head_tested_body(ast, prefix, cond, body);
        let always = ast.add_expr(Expr::Bool(true));
        *s = Stmt::While {
            cond: always,
            body: Box::new(new_body),
        };
    }
    true
}

/// `do … while` is left (its `continue` skips to the test, which the
/// head-tested form would not re-run).
pub(super) fn rewrite_do_while(
    ast: &mut Ast,
    cond: ExprId,
    body: &mut Stmt,
    cx: &Cx,
    log: &mut LiftLog,
) -> bool {
    !expr_has_await(ast, cond) && rewrite_stmt(ast, body, cx, log)
}

/// `switch (await x)` — the discriminant is evaluated once; case values
/// with awaits keep the refusal.
pub(super) fn rewrite_switch(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::Switch {
        scrutinee,
        cases,
        default,
        ..
    } = s
    else {
        unreachable!("rewrite_switch on a switch")
    };
    if cases.iter().any(|c| expr_has_await(ast, c.value)) {
        return false;
    }
    for c in cases.iter_mut() {
        if !rewrite_list(ast, &mut c.body, cx, log) {
            return false;
        }
    }
    if let Some(d) = default.as_mut()
        && !rewrite_list(ast, d, cx, log)
    {
        return false;
    }
    let scrutinee = *scrutinee;
    let Some(prefix) = lift_root(ast, scrutinee, cx, log) else {
        return false;
    };
    with_prefix(s, prefix);
    true
}
