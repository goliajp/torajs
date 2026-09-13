//! The body rewrite of [`super::desugar_async_sm`] — every await
//! becomes a suspend point the state machine can take. Statement-level
//! awaits (`await e;` / `let v = await e;`) become `Stmt::Yield` /
//! `YieldInto` directly; an await INSIDE an expression is first lifted
//! to statement level by `desugar_async_sm_lift` (RFC 20260805 D1b).
//! Split from the parent when the lift landed (file-size line).

use super::desugar_async_sm::{await_inner, expr_has_await, stmt_has_await};
use super::desugar_async_sm_lift::{LiftLog, can_lift, lift, stable_names};
use super::{Ast, Stmt};
use std::collections::HashSet;

/// Rewrite every await in `body` into a yield. Answers false — leaving
/// the arena exactly as it was and `body` for the caller to discard —
/// when any await sits somewhere neither `Stmt::Yield` nor the lift can
/// reach.
pub(super) fn rewrite_body(ast: &mut Ast, body: &mut [Stmt]) -> bool {
    let stable = stable_names(ast);
    let mut log = LiftLog::default();
    let ok = rewrite_list(ast, body, &stable, &mut log);
    if !ok {
        log.undo(ast);
    }
    ok
}

fn rewrite_list(
    ast: &mut Ast,
    body: &mut [Stmt],
    stable: &HashSet<String>,
    log: &mut LiftLog,
) -> bool {
    body.iter_mut().all(|s| rewrite_stmt(ast, s, stable, log))
}

/// Lift the awaits inside `root` (an expression the statement evaluates
/// in full) ahead of the statement. `Some(prefix)` = the statements to
/// run first; `None` = an await sits where the lift does not go.
fn lift_root(
    ast: &mut Ast,
    root: crate::ast::ExprId,
    stable: &HashSet<String>,
    log: &mut LiftLog,
) -> Option<Vec<Stmt>> {
    let mut prefix = Vec::new();
    if !expr_has_await(ast, root) {
        return Some(prefix);
    }
    if !can_lift(ast, root) {
        return None;
    }
    lift(ast, root, &mut prefix, stable, log);
    Some(prefix)
}

/// `prefix` ahead of `s`, as one `Multi` (or `s` alone when nothing
/// was lifted).
fn with_prefix(s: &mut Stmt, mut prefix: Vec<Stmt>) {
    if prefix.is_empty() {
        return;
    }
    let own = std::mem::replace(s, Stmt::Multi(Vec::new()));
    prefix.push(own);
    *s = Stmt::Multi(prefix);
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, stable: &HashSet<String>, log: &mut LiftLog) -> bool {
    match s {
        // `await e;` — and `f(await e)`, lifted.
        Stmt::Expr(eid) => {
            let eid = *eid;
            if let Some(inner) = await_inner(ast, eid) {
                let Some(prefix) = lift_root(ast, inner, stable, log) else {
                    return false;
                };
                *s = Stmt::Yield(inner);
                with_prefix(s, prefix);
                return true;
            }
            let Some(prefix) = lift_root(ast, eid, stable, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // `let v = await e;` — and `let v = f(await e)`, lifted.
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let init = *init;
            if let Some(inner) = await_inner(ast, init) {
                let Some(prefix) = lift_root(ast, inner, stable, log) else {
                    return false;
                };
                ast.await_yield_values.insert(inner);
                *s = Stmt::YieldInto {
                    var: name.clone(),
                    type_ann: type_ann.clone(),
                    value: inner,
                };
                with_prefix(s, prefix);
                return true;
            }
            let Some(prefix) = lift_root(ast, init, stable, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // `return await e` / `return f(await e)` / `throw await e`.
        Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            let e = *e;
            let Some(prefix) = lift_root(ast, e, stable, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        Stmt::Return(None) => true,
        // The condition is evaluated once, before the branches: its
        // awaits lift ahead of the whole `if`.
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond = *cond;
            if !rewrite_stmt(ast, then_branch, stable, log)
                || !else_branch
                    .as_deref_mut()
                    .is_none_or(|b| rewrite_stmt(ast, b, stable, log))
            {
                return false;
            }
            let Some(prefix) = lift_root(ast, cond, stable, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // A loop head re-evaluates per iteration — a hoist before the
        // loop is not that; the body is fair game.
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            !expr_has_await(ast, *cond) && rewrite_stmt(ast, body, stable, log)
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(ast, body, stable, log),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => rewrite_list(ast, stmts, stable, log),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(ast, body, stable, log)
                && rewrite_list(ast, catch_body, stable, log)
                && finally_body
                    .as_deref_mut()
                    .is_none_or(|f| rewrite_list(ast, f, stable, log))
        }
        // Everything else blocks the move if it holds an await; the
        // conservative direction is refusing to move, never dropping a
        // suspend point. `for` heads, `switch` discriminants and the
        // for-of/for-in iterables all land here.
        other => !stmt_has_await(ast, other),
    }
}
