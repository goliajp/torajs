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

mod class_heads;
mod loops;

use super::desugar_async_sm::{await_inner, expr_has_await, stmt_has_await};
use super::desugar_async_sm_lift::{LiftLog, can_lift, lift, stable_names};
use super::{Ast, ExprId, Stmt};
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
        Stmt::While { .. } => loops::rewrite_while(ast, s, cx, log),
        Stmt::DoWhile { .. } => loops::rewrite_do_while(ast, s, cx, log),
        Stmt::For { .. } => loops::rewrite_for(ast, s, cx, log),
        Stmt::Switch { .. } => loops::rewrite_switch(ast, s, cx, log),
        Stmt::Labeled { .. } => loops::rewrite_labeled(ast, s, cx, log),
        Stmt::ClassDecl { .. } => class_heads::rewrite_class(ast, s, cx, log),
        // `for (x of src)` / `for await (x of src)` — the parser hoisted a
        // non-identifier source into a `let` ahead of the loop, so an
        // await in it lifts through the LetDecl arm; the body's awaits
        // become the loop's suspends, and `desugar_generators_forof`
        // turns the loop into the manual iterator protocol (a `for
        // await`'s step is a suspend of its own, RFC 20260915 blade 4).
        // A for-in's object is a name the parser bound ahead of it, and
        // its body suspends like any loop's (`desugar_generators_forin`).
        Stmt::ForOf { body, .. } => rewrite_stmt(ast, body, cx, log),
        Stmt::ForOfSplitIter { .. } => loops::rewrite_split_for_of(ast, s, cx, log),
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
        // suspend point.
        other => !stmt_has_await(ast, other),
    }
}
