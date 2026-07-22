//! Guard-dominated bounds-check elision (RFC 20260708-typed-arr-
//! oob-read perf follow-up).
//!
//! The typed index lane's OOB bounds branch (chunk 2) costs a len
//! load + two compares + a slot round-trip per read — measured
//! +17-47% on the index-heavy bench loops (`sum += xs[i]` under
//! `i < xs.length`), because LLVM cannot hoist the len load past
//! the raw-pointer LoadDyn aliasing wall. This module restores the
//! direct load for exactly the textbook-provable case:
//!
//! ```text
//! for (…; i < xs.length; …) {   while (i < xs.length) {
//!   … xs[i] …                     … xs[i] …    // proven
//!   i = i + 1;                    i = i + 1;   // evicts the pair
//! }                             }
//! ```
//!
//! - `guard_pair` recognizes the `Ident(i) < Ident(xs).length`
//!   condition; the loop lowerers push the pair around the body.
//! - `evict_tainted` runs per top-level body statement (Block arm):
//!   a statement whose subtree writes `i` (assign / post-incr),
//!   rebinds `xs`, or lets `xs` escape as a value (call argument,
//!   method receiver, store — anything but an `xs[…]` read or
//!   `xs.length`) evicts the pair BEFORE the statement lowers, so
//!   every later read keeps the checked branch. `xs[…] = v` elem
//!   writes stay safe (grow-only: length never shrinks through the
//!   index-assign lane), and the guard re-proves on every
//!   iteration, so `i` writes AFTER the reads (the loop step) are
//!   fine — the eviction is positional within the body sequence.
//! - `is_proven` answers the index lane's elision query for the
//!   exact `xs[i]` shape.
//!
//! Unknown statement / expression shapes taint conservatively —
//! a missed elision costs one predictable branch, never
//! correctness.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa_lower::LowerCtx;

/// `Ident(i) < Member(Ident(xs), "length")` → `(i, xs)`.
pub(crate) fn guard_pair(ast: &Ast, cond: ExprId) -> Option<(String, String)> {
    let Expr::BinOp {
        op: crate::ast::BinOp::Lt,
        left,
        right,
    } = ast.get_expr(cond)
    else {
        return None;
    };
    let Expr::Ident(i) = ast.get_expr(*left) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*right) else {
        return None;
    };
    if name != "length" {
        return None;
    }
    let Expr::Ident(xs) = ast.get_expr(*obj) else {
        return None;
    };
    Some((i.clone(), xs.clone()))
}

/// True when `obj[index]` is exactly `xs[i]` for a pair on the
/// proven stack.
pub(crate) fn is_proven(ctx: &LowerCtx<'_>, obj: ExprId, index: ExprId) -> bool {
    if ctx.bounds_proven.is_empty() {
        return false;
    }
    let Expr::Ident(xs) = ctx.ast.get_expr(obj) else {
        return false;
    };
    let Expr::Ident(i) = ctx.ast.get_expr(index) else {
        return false;
    };
    ctx.bounds_proven
        .iter()
        .any(|(pi, pxs)| pi == i && pxs == xs)
}

/// Evict every proven pair the statement's subtree taints. Runs
/// before the statement lowers (Block arm) so reads inside a
/// tainting statement are NOT elided.
pub(crate) fn evict_tainted(ctx: &mut LowerCtx<'_>, s: &Stmt) {
    ctx.bounds_proven = std::mem::take(&mut ctx.bounds_proven)
        .into_iter()
        .filter(|(i, xs)| !stmt_taints(ctx.ast, s, i, xs))
        .collect();
}

fn stmt_taints(ast: &Ast, s: &Stmt, i: &str, xs: &str) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_taints(ast, *e, i, xs),
        Stmt::Return(opt) => opt.is_some_and(|e| expr_taints(ast, e, i, xs)),
        Stmt::LetDecl { name, init, .. } => {
            // shadowing the guard names kills the proof too.
            name == i || name == xs || expr_taints(ast, *init, i, xs)
        }
        Stmt::Block(stmts) => stmts.iter().any(|s| stmt_taints(ast, s, i, xs)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_taints(ast, *cond, i, xs)
                || stmt_taints(ast, then_branch, i, xs)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_taints(ast, e, i, xs))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_taints(ast, *cond, i, xs) || stmt_taints(ast, body, i, xs)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_taints(ast, s, i, xs))
                || cond.is_some_and(|e| expr_taints(ast, e, i, xs))
                || step.is_some_and(|e| expr_taints(ast, e, i, xs))
                || stmt_taints(ast, body, i, xs)
        }
        Stmt::Break(_) | Stmt::Continue(_) => false,
        // A `label: stmt` taints iff its body does — the label is
        // control-flow only, orthogonal to index/array mutation.
        Stmt::Labeled { body, .. } => stmt_taints(ast, body, i, xs),
        // Anything else (try / switch / for-of / yield-into / …) —
        // conservative taint.
        _ => true,
    }
}

fn expr_taints(ast: &Ast, eid: ExprId, i: &str, xs: &str) -> bool {
    match ast.get_expr(eid) {
        // bare `xs` in a value position escapes (call arg, store,
        // literal elem, method receiver via the Member arm below).
        Expr::Ident(n) => n == xs,
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null | Expr::This => false,
        Expr::Index { obj, index } => {
            let obj_safe = matches!(ast.get_expr(*obj), Expr::Ident(n) if n == xs);
            (!obj_safe && expr_taints(ast, *obj, i, xs)) || expr_taints(ast, *index, i, xs)
        }
        Expr::Member { obj, name } => {
            let obj_safe =
                name == "length" && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == xs);
            !obj_safe && expr_taints(ast, *obj, i, xs)
        }
        Expr::Assign { target, value } => {
            let target_writes = match ast.get_expr(*target) {
                Expr::Ident(n) => n == i || n == xs,
                // `xs[k] = v` elem write is grow-only — len never
                // shrinks through the index-assign lane; recurse
                // only into the key expression.
                Expr::Index { obj, index } => {
                    let obj_safe = matches!(ast.get_expr(*obj), Expr::Ident(n) if n == xs);
                    (!obj_safe && expr_taints(ast, *obj, i, xs)) || expr_taints(ast, *index, i, xs)
                }
                _ => expr_taints(ast, *target, i, xs),
            };
            target_writes || expr_taints(ast, *value, i, xs)
        }
        Expr::PostIncr { target, .. } => match ast.get_expr(*target) {
            Expr::Ident(n) => n == i || n == xs,
            _ => expr_taints(ast, *target, i, xs),
        },
        Expr::BinOp { left, right, .. }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Sequence { left, right } => {
            expr_taints(ast, *left, i, xs) || expr_taints(ast, *right, i, xs)
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => expr_taints(ast, *expr, i, xs),
        Expr::Call { callee, args } => {
            expr_taints(ast, *callee, i, xs) || args.iter().any(|a| expr_taints(ast, *a, i, xs))
        }
        Expr::Array(elems) => elems.iter().any(|e| expr_taints(ast, *e, i, xs)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_taints(ast, *cond, i, xs)
                || expr_taints(ast, *then_branch, i, xs)
                || expr_taints(ast, *else_branch, i, xs)
        }
        // Unknown shapes (object literals, closures capturing xs,
        // optional chains, …) — conservative taint.
        _ => true,
    }
}
