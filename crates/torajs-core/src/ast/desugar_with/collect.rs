//! The two walks the `with` desugar runs over a body: which names the
//! body binds LEXICALLY (those shadow the object), and where every
//! remaining identifier occurrence sits (which decides its rewrite).
//!
//! Both are deliberately conservative in the same direction: a shape
//! this file does not recognise becomes a `Position::Refused`, so an
//! unhandled corner is a diagnostic rather than a name that quietly
//! resolved to the lexical binding.

use std::collections::HashSet;

use super::Position;
use super::walk::{expr_children, stmt_children_ref};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Collect the body's lexical binders into `bound` and every
/// identifier occurrence into `sites`.
///
/// `var` names are NOT collected: they hoist past the object record,
/// so `with (o) { var v = 1 }` still means `o.v` when `o` has `v`.
/// `let` / `const` / `class` / catch parameters are, because their
/// records sit in front of the object's.
pub(crate) fn collect_stmt(
    ast: &Ast,
    s: &Stmt,
    bound: &mut HashSet<String>,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match s {
        Stmt::LetDecl {
            name, init, is_var, ..
        } => {
            if *is_var {
                // `var` does NOT shadow the object — it hoists to the
                // function scope, which sits BEHIND the object record.
                // So `with (o) { var v = 2 }` writes `o.v` when `o`
                // carries `v`, and leaves the hoisted binding
                // undefined (bun: `2 undefined`; the unrewritten
                // reading is `1 2`). The initialiser is an assignment
                // evaluated in the with scope, so it belongs to the
                // write knife — refused until then rather than
                // silently landing on the hoisted binding.
                refuse(err, "a `var` declaration");
            } else {
                bound.insert(name.clone());
            }
            collect_expr(ast, *init, sites, err);
        }
        Stmt::ClassDecl { name, .. } => {
            bound.insert(name.clone());
            refuse(err, "a class declaration");
        }
        Stmt::FnDecl { name, .. } => {
            bound.insert(name.clone());
            // §14.11 with a function inside it: the body's free names
            // resolve when the function is CALLED, so the object has
            // to be captured by the closure — RFC 20260814 刀 4.
            refuse(err, "a nested function body");
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => collect_expr(ast, *e, sites, err),
        Stmt::Return(o) => {
            if let Some(e) = o {
                collect_expr(ast, *e, sites, err);
            }
        }
        Stmt::YieldInto { value, .. } => collect_expr(ast, *value, sites, err),
        Stmt::If { cond, .. } | Stmt::While { cond, .. } | Stmt::DoWhile { cond, .. } => {
            collect_expr(ast, *cond, sites, err);
        }
        Stmt::Switch {
            scrutinee, cases, ..
        } => {
            collect_expr(ast, *scrutinee, sites, err);
            for c in cases {
                collect_expr(ast, c.value, sites, err);
            }
        }
        Stmt::For { cond, step, .. } => {
            if let Some(c) = cond {
                collect_expr(ast, *c, sites, err);
            }
            if let Some(st) = step {
                collect_expr(ast, *st, sites, err);
            }
        }
        Stmt::ForOf {
            var_name,
            src_ident,
            i_ident,
            elem_expr,
            ..
        } => {
            // The parser already hoisted the source to `src_ident`
            // and minted the counter, so all three names are the
            // loop's own — bound, not free.
            bound.insert(var_name.clone());
            bound.insert(src_ident.clone());
            bound.insert(i_ident.clone());
            collect_expr(ast, *elem_expr, sites, err);
        }
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            ..
        } => {
            bound.insert(var_name.clone());
            collect_expr(ast, *parent, sites, err);
            collect_expr(ast, *sep, sites, err);
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(p) = catch_param {
                bound.insert(p.clone());
            }
        }
        Stmt::UsingDecl { name, init, .. } => {
            bound.insert(name.clone());
            collect_expr(ast, *init, sites, err);
        }
        Stmt::Block(_) | Stmt::Multi(_) | Stmt::Labeled { .. } => {}
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::TypeDecl { .. } => {}
        Stmt::ImportDecl { .. } | Stmt::ExportDecl { .. } => {}
    }
    for child in stmt_children_ref(s) {
        collect_stmt(ast, child, bound, sites, err);
    }
}

fn refuse(err: &mut Option<String>, what: &'static str) {
    err.get_or_insert(format!(
        "{what} inside a `with` body is not yet supported \
         (RFC 20260814, ES §14.11)"
    ));
}

/// Walk one expression subtree, recording each `Ident` with the
/// position that decides its rewrite.
///
/// A bare `Ident` records ITSELF as a read, rather than being recorded
/// by whoever passed it down. 刀 1 did the latter, and so never saw an
/// identifier that is a statement's whole expression: `with (o) {
/// return x }`, `if (x)`, `throw x` all left `x` resolving lexically
/// while the object carried it. A node that owns its own site cannot
/// be missed by a caller that forgets to announce it.
///
/// The positions that are NOT plain reads (callee, assignment target,
/// the sole operand of `typeof` / `++`) are still recorded by the
/// parent, which is the only node that knows which one applies — and
/// the parent does not then descend into that child, so no site is
/// recorded twice.
fn collect_expr(
    ast: &Ast,
    eid: ExprId,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match ast.get_expr(eid) {
        Expr::Ident(_) => {
            sites.push((eid, Position::Read));
            return;
        }
        Expr::Call { callee, args } => {
            if matches!(ast.get_expr(*callee), Expr::Ident(_)) {
                sites.push((*callee, Position::Callee(eid)));
            } else {
                collect_expr(ast, *callee, sites, err);
            }
            for a in args {
                collect_expr(ast, *a, sites, err);
            }
            return;
        }
        Expr::Assign { target, value } => {
            let Expr::Ident(tn) = ast.get_expr(*target) else {
                collect_expr(ast, *target, sites, err);
                collect_expr(ast, *value, sites, err);
                return;
            };
            // `n op= v` reaches here as `n = n op v` with a CLONED
            // left operand (the parser's compound desugar). That clone
            // is the same reference the write resolves, not a second
            // one, so it must not get a guard of its own — the assign
            // rewrite fills it in per branch and §9.1.1.2.1 HasBinding
            // stays evaluated once, as the spec's single
            // ResolveBinding requires.
            let compound_lhs = match ast.get_expr(*value) {
                Expr::BinOp { left, .. } if matches!(ast.get_expr(*left), Expr::Ident(ln) if ln == tn) => {
                    Some(*left)
                }
                _ => None,
            };
            sites.push((*target, Position::Assign(eid, compound_lhs)));
            match (compound_lhs, ast.get_expr(*value)) {
                (Some(_), Expr::BinOp { right, .. }) => {
                    collect_expr(ast, *right, sites, err);
                }
                _ => collect_expr(ast, *value, sites, err),
            }
            return;
        }
        Expr::PostIncr { target, .. } => {
            if matches!(ast.get_expr(*target), Expr::Ident(_)) {
                sites.push((*target, Position::Wrapping(eid)));
            } else {
                collect_expr(ast, *target, sites, err);
            }
            return;
        }
        Expr::TypeOf { expr } => {
            if matches!(ast.get_expr(*expr), Expr::Ident(_)) {
                sites.push((*expr, Position::Wrapping(eid)));
            } else {
                collect_expr(ast, *expr, sites, err);
            }
            return;
        }
        Expr::Delete { expr } => {
            if matches!(ast.get_expr(*expr), Expr::Ident(_)) {
                sites.push((*expr, Position::Refused("`delete`")));
            } else {
                collect_expr(ast, *expr, sites, err);
            }
            return;
        }
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {
            // Same reason as a nested `function`: the body's names
            // resolve at call time, so the object has to be captured
            // (RFC 20260814 刀 4).
            refuse(err, "a nested function body");
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, sites, err);
    }
}
