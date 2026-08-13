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
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
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
        _ => {}
    }
}

fn collect_assigned_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                out.insert(n.clone());
            } else {
                collect_assigned_in_expr(ast, *target, out);
            }
            collect_assigned_in_expr(ast, *value, out);
        }
        Expr::PostIncr { target, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                out.insert(n.clone());
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_assigned_in_stmt(ast, s, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_assigned_in_expr(ast, *left, out);
            collect_assigned_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => collect_assigned_in_expr(ast, *expr, out),
        Expr::InstanceOf { expr, rhs } => {
            collect_assigned_in_expr(ast, *expr, out);
            collect_assigned_in_expr(ast, *rhs, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_assigned_in_expr(ast, *obj, out)
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            collect_assigned_in_expr(ast, *obj, out);
            collect_assigned_in_expr(ast, *index, out);
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            collect_assigned_in_expr(ast, *callee, out);
            for a in args {
                collect_assigned_in_expr(ast, *a, out);
            }
        }
        Expr::Array(els) => {
            for e in els {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_assigned_in_expr(ast, *cond, out);
            collect_assigned_in_expr(ast, *then_branch, out);
            collect_assigned_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs }
        | Expr::Sequence {
            left: lhs,
            right: rhs,
        } => {
            collect_assigned_in_expr(ast, *lhs, out);
            collect_assigned_in_expr(ast, *rhs, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_assigned_in_expr(ast, *e, out);
            }
        }
        _ => {}
    }
}
