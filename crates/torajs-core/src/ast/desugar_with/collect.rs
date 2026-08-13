//! Where every FREE identifier of a `with` body sits — which is what
//! decides its rewrite. The shadowing question is [`super::scope`]'s;
//! this file asks the position question and records the answer.
//!
//! The walk is deliberately conservative in one direction: a shape it
//! does not recognise sets `err` instead of being skipped, so an
//! unhandled corner is a diagnostic rather than a name that quietly
//! resolved to the lexical binding.

use super::Position;
use super::scope::Scope;
use super::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Entry point: record every free identifier occurrence of one `with`
/// body, nested function bodies included.
pub(crate) fn collect_body(
    ast: &Ast,
    body: &[Stmt],
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    let scope = Scope::with_body(body);
    for s in body {
        collect_stmt(ast, s, &scope, sites, err);
    }
}

fn collect_stmt(
    ast: &Ast,
    s: &Stmt,
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match s {
        Stmt::LetDecl {
            init, is_var: true, ..
        } if !scope.in_nested_fn() && !matches!(ast.get_expr(*init), Expr::Uninit) => {
            // `var` does NOT shadow the object — it hoists to the
            // enclosing function's scope, which sits BEHIND the object
            // record — while its initialiser IS evaluated in front of
            // it. `super::var_split` has already separated the two for
            // every `var` it can reach, so an initialiser still
            // standing here is one it deliberately left: several
            // declarators in one `for` initialiser, which arrive as a
            // `Multi` in the loop's init slot. Loud rather than
            // silently landing on the hoisted binding.
            refuse(err, "a multi-declarator `var` in a `for` initialiser");
        }
        Stmt::ClassDecl { .. } => refuse(err, "a class declaration"),
        Stmt::FnDecl { params, body, .. } => {
            collect_fn(ast, params, body, scope, sites, err);
            return;
        }
        _ => {}
    }
    for e in stmt_exprs(s) {
        collect_expr(ast, e, scope, sites, err);
    }
    for child in stmt_children_ref(s) {
        collect_stmt(ast, child, scope, sites, err);
    }
}

/// A function nested in the body: its free names resolve when it is
/// CALLED, so they are guarded exactly like the body's own and the
/// `with` binding is captured by the closure — which works because the
/// binding is an ordinary block-scoped `let` that the capture analysis
/// downstream has no reason to treat specially.
///
/// A parameter default is walked in the FUNCTION's scope, not the
/// body's, even though tr evaluates defaults at the call site: the
/// guard it grows names the with binding, so a default is only
/// well-formed while that call site is still inside the block. One
/// that escapes the block fails loudly on the unknown binding rather
/// than answering the wrong name.
fn collect_fn(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    let child = scope.nested_fn(params, body);
    for p in params {
        if let Some(d) = p.default {
            collect_expr(ast, d, &child, sites, err);
        }
    }
    for s in body {
        collect_stmt(ast, s, &child, sites, err);
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
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            if !scope.shadows(n) {
                sites.push((eid, Position::Read));
            }
            return;
        }
        Expr::Call { callee, args } => {
            match ast.get_expr(*callee) {
                Expr::Ident(n) => {
                    if !scope.shadows(n) {
                        sites.push((*callee, Position::Callee(eid)));
                    }
                }
                _ => collect_expr(ast, *callee, scope, sites, err),
            }
            for a in args {
                collect_expr(ast, *a, scope, sites, err);
            }
            return;
        }
        Expr::Assign { target, value } => {
            let Expr::Ident(tn) = ast.get_expr(*target) else {
                collect_expr(ast, *target, scope, sites, err);
                collect_expr(ast, *value, scope, sites, err);
                return;
            };
            if scope.shadows(tn) {
                collect_expr(ast, *value, scope, sites, err);
                return;
            }
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
                    collect_expr(ast, *right, scope, sites, err);
                }
                _ => collect_expr(ast, *value, scope, sites, err),
            }
            return;
        }
        Expr::PostIncr { target, .. } => {
            wrapping_operand(ast, eid, *target, scope, sites, err);
            return;
        }
        Expr::TypeOf { expr } => {
            wrapping_operand(ast, eid, *expr, scope, sites, err);
            return;
        }
        Expr::Delete { expr } => {
            wrapping_operand(ast, eid, *expr, scope, sites, err);
            return;
        }
        Expr::ArrowFn { params, body, .. } => {
            // A function EXPRESSION (the parser gives `function () {}`
            // and `() => {}` the same node): its free names resolve at
            // call time, through the object captured by the closure.
            collect_fn(ast, params, body, scope, sites, err);
            return;
        }
        Expr::Closure { .. } => {
            // Post-`lift_arrow_fns` shape. This pass runs long before
            // that, so reaching one means the pipeline order moved.
            refuse(err, "an already-lifted closure");
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, scope, sites, err);
    }
}

/// The sole operand of `typeof` / `++` / `--`, whose WRAPPING node is
/// what gets replaced.
fn wrapping_operand(
    ast: &Ast,
    outer: ExprId,
    operand: ExprId,
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match ast.get_expr(operand) {
        Expr::Ident(n) => {
            if !scope.shadows(n) {
                sites.push((operand, Position::Wrapping(outer)));
            }
        }
        _ => collect_expr(ast, operand, scope, sites, err),
    }
}
