//! The assigned-name half of the escape-capture pre-pass: which
//! bindings does a body (and every lifted closure it constructs)
//! WRITE?
//!
//! Separate from the parent's question — what does a closure CAPTURE
//! — and only joined at the end, where a captured name that is also
//! written is the one that needs a box. Split out at the 500-line
//! limit; a child module reaches the parent's private items directly,
//! so nothing changed visibility.

use std::collections::HashSet;

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

use super::{ClosureScan, collect_closure_captures_in_stmt};

/// RFC 20260710 — every name that is the target of an `Assign` /
/// `PostIncr` anywhere in the program, including lifted closure
/// bodies (all top-level FnDecls, so a closure writing its captured
/// binding is covered by the top-level walk). By-name approximation,
/// same precision as the escape collector: over-collection promotes
/// a never-written same-named capture to a box — one extra
/// indirection, semantics unchanged.
/// Assigned names visible to ONE body's escape-capture promotion: the
/// body's own assignments plus those of every lifted closure it
/// (transitively) constructs — the only bodies that can write its
/// captured bindings. The old program-wide collection keyed by bare
/// name, so an unrelated same-named toplevel assignment promoted a
/// never-written param to a capture box (RFC 20260731-mono-closure-
/// clone 刀 2 — the scope pollution that amplified the shared-decl
/// layout bug into a UAF).
pub(crate) fn collect_assigned_names_scoped<'s>(
    ast: &Ast,
    body: impl Iterator<Item = &'s Stmt>,
    seed_fns: &[String],
) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in body {
        collect_assigned_in_stmt(ast, s, &mut out);
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = seed_fns.to_vec();
    // RFC 20260912 — a generator's iterator methods write the captures
    // of its wrapper closure (they read them through its env), so a
    // wrapper's bodies include those methods.
    let gen_env_methods = crate::ast::gen_env_methods_by_wrapper(ast);
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if let Some(methods) = gen_env_methods.get(&name) {
            pending.extend(methods.iter().cloned());
        }
        let Some(fn_body) = ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl { name: n, body, .. } if *n == name => Some(body),
            _ => None,
        }) else {
            continue;
        };
        let mut scan = ClosureScan::default();
        for s in fn_body {
            collect_assigned_in_stmt(ast, s, &mut out);
            collect_closure_captures_in_stmt(ast, s, &mut scan);
        }
        pending.extend(scan.fn_names);
    }
    out
}

/// The names `eid` assigns, closures it constructs included.
pub(crate) fn expr_assigned_names(ast: &Ast, eid: ExprId) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_assigned_in_expr(ast, eid, &mut out);
    out
}

fn collect_assigned_in_stmt(ast: &Ast, s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_assigned_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_assigned_in_expr(ast, *value, out),
        Stmt::Return(Some(eid)) => collect_assigned_in_expr(ast, *eid, out),
        Stmt::LetDecl { init, .. } => collect_assigned_in_expr(ast, *init, out),
        Stmt::FnDecl { body, .. } => {
            for s in body {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_assigned_in_stmt(ast, eb, out);
            }
        }
        Stmt::Labeled { body, .. } => {
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_assigned_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_assigned_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_assigned_in_expr(ast, *st, out);
            }
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            collect_assigned_in_expr(ast, *elem_expr, out);
            if let Some(fo) = forin_obj {
                collect_assigned_in_expr(ast, *fo, out);
            }
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            collect_assigned_in_expr(ast, *parent, out);
            collect_assigned_in_expr(ast, *sep, out);
            collect_assigned_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            collect_assigned_in_expr(ast, *scrutinee, out);
            for c in cases {
                for s in &c.body {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body.iter().chain(catch_body.iter()) {
                collect_assigned_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_assigned_in_stmt(ast, s, out);
                }
            }
        }
        // Any other statement: its expressions and sub-statements
        // through the shared enumerators, so a statement shape nobody
        // listed above cannot hide a write either.
        _ => {
            for e in stmt_exprs(s) {
                collect_assigned_in_expr(ast, e, out);
            }
            for c in stmt_children_ref(s) {
                collect_assigned_in_stmt(ast, c, out);
            }
        }
    }
}

/// Every identifier `eid` (or a function expression inside it) assigns.
/// The walk below the two arms that name something is the shared
/// child enumerator, so no expression shape can hide an assignment:
/// the hand-listed walk it replaces skipped `delete` and `new` with a
/// computed callee, and a closure writing through
/// `delete super[x = 1]` left `x` looking never-written (snapshotted,
/// the write lost — staging/sm/class/superPropDelete).
fn collect_assigned_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                out.insert(n.clone());
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_assigned_in_expr(ast, c, out);
    }
}
