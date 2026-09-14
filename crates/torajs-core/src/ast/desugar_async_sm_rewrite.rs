//! The body rewrite of [`super::desugar_async_sm`] — every await
//! becomes a suspend point the state machine can take. Statement-level
//! awaits (`await e;` / `let v = await e;`) become `Stmt::Yield` /
//! `YieldInto` directly; an await INSIDE an expression is first lifted
//! to statement level by `desugar_async_sm_lift` (RFC 20260805 D1b).
//! Split from the parent when the lift landed (file-size line).
//!
//! Two modes (D4): an async FUNCTION's body yields the awaited value
//! itself; an async GENERATOR's body yields an `__AwaitMark` around it,
//! so the driver can tell the body's own yields from its awaits
//! (`desugar_generators_async`).

use super::desugar_async_sm::{await_inner, expr_has_await, stmt_has_await};
use super::desugar_async_sm_lift::{LiftLog, can_lift, lift, stable_names};
use super::{Ast, Expr, ExprId, Stmt, UnaryOp};
use std::collections::HashSet;

/// Rewrite every await in `body` into a yield. Answers false — leaving
/// the arena exactly as it was and `body` for the caller to discard —
/// when any await sits somewhere neither `Stmt::Yield` nor the lift can
/// reach.
pub(super) fn rewrite_body(ast: &mut Ast, body: &mut [Stmt]) -> bool {
    rewrite_body_mode(ast, body, false)
}

/// D4 — the same rewrite for an async GENERATOR body: every await
/// yields an `__AwaitMark`, and the body's own `yield`s stay, their
/// operands lifted like any statement's.
pub(super) fn rewrite_body_marked(ast: &mut Ast, body: &mut [Stmt]) -> bool {
    rewrite_body_mode(ast, body, true)
}

fn rewrite_body_mode(ast: &mut Ast, body: &mut [Stmt], mark: bool) -> bool {
    let cx = Cx {
        stable: stable_names(ast),
        mark,
    };
    let mut log = LiftLog::default();
    let ok = rewrite_list(ast, body, &cx, &mut log);
    if !ok {
        log.undo(ast);
    }
    ok
}

/// What one body's rewrite shares.
struct Cx {
    stable: HashSet<String>,
    mark: bool,
}

/// The yield operand for `await inner`: the value itself in an async
/// function, the mark in an async generator.
fn await_operand(ast: &mut Ast, inner: ExprId, cx: &Cx) -> ExprId {
    if cx.mark {
        super::desugar_generators_async::mint_await_mark(ast, inner)
    } else {
        inner
    }
}

fn rewrite_list(ast: &mut Ast, body: &mut [Stmt], cx: &Cx, log: &mut LiftLog) -> bool {
    body.iter_mut().all(|s| rewrite_stmt(ast, s, cx, log))
}

/// Lift the awaits inside `root` (an expression the statement evaluates
/// in full) ahead of the statement. `Some(prefix)` = the statements to
/// run first; `None` = an await sits where the lift does not go.
fn lift_root(ast: &mut Ast, root: ExprId, cx: &Cx, log: &mut LiftLog) -> Option<Vec<Stmt>> {
    let mut prefix = Vec::new();
    if !expr_has_await(ast, root) {
        return Some(prefix);
    }
    if !can_lift(ast, root) {
        return None;
    }
    lift(ast, root, &mut prefix, &cx.stable, log, cx.mark);
    // A guarded position's statement form carries its awaits inside
    // `if` branches — rewrite the prefix like any other body.
    if !rewrite_list(ast, &mut prefix, cx, log) {
        return None;
    }
    Some(prefix)
}

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

/// `for (init; c; step)` — the `while` head form for `c`; an await in
/// `init` or `step` keeps the refusal (init sits in the loop's own
/// scope, step runs after `continue`).
fn rewrite_for(
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
        *body = Box::new(head_tested_body(ast, prefix, c, old));
        *cond = None;
    }
    true
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    match s {
        // `await e;` — and `f(await e)`, lifted.
        Stmt::Expr(eid) => {
            let eid = *eid;
            if let Some(inner) = await_inner(ast, eid) {
                let Some(prefix) = lift_root(ast, inner, cx, log) else {
                    return false;
                };
                let operand = await_operand(ast, inner, cx);
                *s = Stmt::Yield(operand);
                with_prefix(s, prefix);
                return true;
            }
            let Some(prefix) = lift_root(ast, eid, cx, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // An async generator's own `yield e` / `let v = yield e` — the
        // operand may hold awaits (`yield await t`).
        Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
            let e = *e;
            let Some(prefix) = lift_root(ast, e, cx, log) else {
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
                let Some(prefix) = lift_root(ast, inner, cx, log) else {
                    return false;
                };
                ast.await_yield_values.insert(inner);
                let operand = await_operand(ast, inner, cx);
                *s = Stmt::YieldInto {
                    var: name.clone(),
                    type_ann: type_ann.clone(),
                    value: operand,
                };
                with_prefix(s, prefix);
                return true;
            }
            let Some(prefix) = lift_root(ast, init, cx, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // `return await e` / `return f(await e)` / `throw await e`.
        Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            let e = *e;
            let Some(prefix) = lift_root(ast, e, cx, log) else {
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
            if !rewrite_stmt(ast, then_branch, cx, log)
                || !else_branch
                    .as_deref_mut()
                    .is_none_or(|b| rewrite_stmt(ast, b, cx, log))
            {
                return false;
            }
            let Some(prefix) = lift_root(ast, cond, cx, log) else {
                return false;
            };
            with_prefix(s, prefix);
            true
        }
        // A loop head re-evaluates per iteration, so its await cannot
        // hoist ahead of the loop: `while (c)` becomes `while (true) {
        // <lift of c>; if (!c') break; body }` — `continue` in the body
        // returns to the head and re-runs the lift. `do … while` is
        // left (its `continue` skips to the test, which this form would
        // not re-run).
        Stmt::While { cond, body } => {
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
        Stmt::DoWhile { cond, body } => {
            !expr_has_await(ast, *cond) && rewrite_stmt(ast, body, cx, log)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => rewrite_for(ast, init.as_deref(), cond, *step, body, cx, log),
        // `switch (await x)` — the discriminant is evaluated once; case
        // values with awaits keep the refusal.
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
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
        Stmt::Labeled { body, .. } => rewrite_stmt(ast, body, cx, log),
        // `for (x of src)` / `for await (x of src)` — the parser hoisted a
        // non-identifier source into a `let` ahead of the loop, so an
        // await in it lifts through the LetDecl arm; the body's awaits
        // become the loop's suspends, and `desugar_generators_forof`
        // turns the loop into the manual iterator protocol (a `for
        // await`'s step is a suspend of its own, RFC 20260915 blade 4).
        // for-in (`forin_obj`) keeps the refusal below — that rewrite
        // has no arm for it.
        Stmt::ForOf {
            forin_obj: None,
            body,
            ..
        } => rewrite_stmt(ast, body, cx, log),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => rewrite_list(ast, stmts, cx, log),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(ast, body, cx, log)
                && rewrite_list(ast, catch_body, cx, log)
                && finally_body
                    .as_deref_mut()
                    .is_none_or(|f| rewrite_list(ast, f, cx, log))
        }
        // Everything else blocks the move if it holds an await; the
        // conservative direction is refusing to move, never dropping a
        // suspend point. `for` heads, `switch` discriminants and the
        // for-of/for-in iterables all land here.
        other => !stmt_has_await(ast, other),
    }
}
