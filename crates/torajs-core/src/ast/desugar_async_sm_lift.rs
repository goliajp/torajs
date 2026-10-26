//! RFC 20260805 D1b — the A-normal lift: an `await` that sits INSIDE an
//! expression is hoisted to statement level, where `Stmt::Yield` can
//! take it.
//!
//! `Stmt::Yield` / `YieldInto` are statement-level, so blade 1 moved a
//! function only when every await was the whole of an expression
//! statement or a let init. `console.log("n1", order, await a, order)`
//! kept the synchronous lowering — where `await` is a settled-value
//! read that never suspends, and a thenable is awaited as itself.
//!
//! For one statement the lift rewrites every await at a position it
//! takes into
//!
//! ```text
//! let __aw_<eid>: any = yield <inner>;        (Stmt::YieldInto)
//! ```
//!
//! ahead of the statement and reads the temp in place, preserving
//! evaluation order (§13.3.8.1 ArgumentListEvaluation and its
//! siblings): every operand the statement evaluates BEFORE the await is
//! hoisted into its own `const __awh_<eid>` first — a bare name
//! included, since the binding may be reassigned by whatever runs while
//! this function is suspended (`order` above reads "s" before the
//! suspension and "sa" after). Literals, `this`, a closure literal, a
//! top-level function or class name and a builtin global stay where
//! they are. A call's receiver stays too (`obj.m(x, await p)` re-reads
//! `obj.m` after resumption — a method call must keep its receiver, and
//! hoisting the member would lose it; the receiver binding being
//! reassigned during the suspension is the recorded deviation).
//!
//! A GUARDED position — the right side of `&&` / `||` / `??`, a
//! ternary's branches — must not evaluate unconditionally, so it takes
//! the statement form instead of a hoist: `let __awg_<eid>: any` plus
//! an `if` whose branches assign it (`c ? await a : b` → `if (c) { g =
//! await a } else { g = b }`), and the node reads the temp. The caller
//! runs the rewrite over that prefix, so the awaits inside the branches
//! become yields like any statement's.
//!
//! Positions the lift does NOT take, leaving the statement — and so the
//! function — on the synchronous lowering as before: an assignment
//! target that is not a name or a member / index over hoistable parts,
//! a `delete` operand, `++` / `--`, optional chains, and a nested
//! function body (its awaits are its own). Loop heads and switch
//! discriminants are the caller's refusal — a hoist before the loop is
//! not a re-evaluation per iteration.
//!
//! The rewrite mutates the arena in place, and the caller decides
//! whether to move the function only after the WHOLE body is rewritten;
//! every mutation is therefore logged in a [`LiftLog`] the caller
//! replays backwards when it gives the body up.

use std::collections::HashSet;

mod guarded;
mod shape;

use super::desugar_async_sm::{await_inner, expr_has_await};
use super::{Ast, Expr, ExprId, Stmt};
use guarded::guarded_to_statements;
pub(super) use shape::can_lift;
use shape::{Shape, shape};

/// Every arena and side-table change the lift made for one body, so a
/// refused move can be undone (`rewrite_body`'s contract).
#[derive(Default)]
pub(super) struct LiftLog {
    exprs: Vec<(ExprId, Expr)>,
    await_reads: Vec<ExprId>,
    yield_values: Vec<ExprId>,
}

impl LiftLog {
    pub(super) fn undo(self, ast: &mut Ast) {
        for (eid, e) in self.exprs.into_iter().rev() {
            ast.exprs[eid.0 as usize] = e;
        }
        for r in self.await_reads {
            ast.await_value_reads.insert(r);
        }
        for v in self.yield_values {
            ast.await_yield_values.remove(&v);
        }
    }
}

/// Names whose bare read is never hoisted: a top-level function or
/// class (a value read of it must stay a name — the direct-call and
/// class lanes key on it), and the builtin globals.
pub(super) fn stable_names(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        if let Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } = s {
            out.insert(name.clone());
        }
    }
    out
}

/// The lift proper — [`can_lift`] must hold for `eid`. Pushes the
/// hoists and yields onto `prefix` in evaluation order and leaves the
/// rewritten expression at `eid`.
pub(super) fn lift(
    ast: &mut Ast,
    eid: ExprId,
    prefix: &mut Vec<Stmt>,
    stable: &HashSet<String>,
    log: &mut LiftLog,
    mark: bool,
) {
    if let Some(inner) = await_inner(ast, eid) {
        if expr_has_await(ast, inner) {
            lift(ast, inner, prefix, stable, log, mark);
        }
        let tmp = format!("__aw_{}", eid.0);
        ast.await_value_reads.remove(&eid);
        log.await_reads.push(eid);
        if ast.await_yield_values.insert(inner) {
            log.yield_values.push(inner);
        }
        // D4 mark mode — inside an async GENERATOR the yield carries
        // an `__AwaitMark` so the driver can tell it from the body's
        // own yields (`desugar_generators_async`).
        let value = if mark {
            super::desugar_generators_async::mint_await_mark(ast, inner)
        } else {
            inner
        };
        prefix.push(Stmt::YieldInto {
            var: tmp.clone(),
            type_ann: Some("any".into()),
            value,
        });
        replace(ast, eid, Expr::Ident(tmp), log);
        return;
    }
    let kids = match shape(ast, eid) {
        Shape::Leaf | Shape::Refuse(_) => return,
        Shape::Ordered(kids) => kids,
        Shape::Guarded(first, rest) => {
            if expr_has_await(ast, first) {
                lift(ast, first, prefix, stable, log, mark);
            }
            if rest.iter().any(|r| expr_has_await(ast, *r)) {
                guarded_to_statements(ast, eid, prefix, log);
            }
            return;
        }
    };
    let Some(last) = kids.iter().rposition(|k| expr_has_await(ast, *k)) else {
        return;
    };
    for (i, k) in kids.iter().enumerate() {
        if i > last {
            break;
        }
        if expr_has_await(ast, *k) {
            lift(ast, *k, prefix, stable, log, mark);
        }
        if i < last {
            hoist(ast, *k, prefix, stable, log);
        }
    }
}

/// Bind an operand evaluated ahead of a suspension to a temp, so the
/// resumed expression reads the value the statement had already
/// produced. A spread hoists its operand and keeps the spread.
fn hoist(
    ast: &mut Ast,
    eid: ExprId,
    prefix: &mut Vec<Stmt>,
    stable: &HashSet<String>,
    log: &mut LiftLog,
) {
    match ast.get_expr(eid) {
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Closure { .. }
        | Expr::ArrowFn { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Elision => {}
        Expr::Ident(n)
            if n.starts_with("__")
                || n == "undefined"
                || n == "arguments"
                || stable.contains(n)
                || super::free_vars_globals::is_global_name(n) => {}
        Expr::Spread { expr } => {
            let inner = *expr;
            hoist(ast, inner, prefix, stable, log);
        }
        _ => {
            let tmp = format!("__awh_{}", eid.0);
            let moved = ast.get_expr(eid).clone();
            let init = ast.add_expr(moved);
            prefix.push(Stmt::LetDecl {
                mutable: false,
                name: tmp.clone(),
                type_ann: None,
                init,
                is_var: false,
            });
            replace(ast, eid, Expr::Ident(tmp), log);
        }
    }
}

fn replace(ast: &mut Ast, eid: ExprId, with: Expr, log: &mut LiftLog) {
    let old = std::mem::replace(&mut ast.exprs[eid.0 as usize], with);
    log.exprs.push((eid, old));
}
