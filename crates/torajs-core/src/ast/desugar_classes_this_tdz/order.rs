//! Which statements of a derived constructor may run before `super(…)`
//! has bound `this`, judged by statement order inside each list.
//!
//! A `super(…);` expression statement that completes normally has
//! bound the binding (§13.3.7.1 step 8), so every later statement of
//! the same list runs bound and needs no check. Lists are taken one at
//! a time: a `try` block, its `catch` and its `finally` are separate,
//! and so is each `switch` case, since control can enter one without
//! having run the one before it.

use crate::ast::desugar_with::walk;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The statement lists `s` holds directly, each to be read in order.
pub(in crate::ast) fn lists(s: &Stmt) -> Vec<&[Stmt]> {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => vec![v.as_slice()],
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut out = vec![body.as_slice(), catch_body.as_slice()];
            if let Some(f) = finally_body {
                out.push(f.as_slice());
            }
            out
        }
        Stmt::Switch { cases, default, .. } => {
            let mut out: Vec<&[Stmt]> = cases.iter().map(|c| c.body.as_slice()).collect();
            if let Some(d) = default {
                out.push(d.as_slice());
            }
            out
        }
        _ => walk::stmt_children_ref(s)
            .into_iter()
            .map(std::slice::from_ref)
            .collect(),
    }
}

/// [`lists`] for a rewrite; a nested declaration's body is its own
/// frame and is not entered.
pub(in crate::ast) fn lists_mut(s: &mut Stmt) -> Vec<&mut [Stmt]> {
    match s {
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => Vec::new(),
        Stmt::Block(v) | Stmt::Multi(v) => vec![v.as_mut_slice()],
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let mut out = vec![body.as_mut_slice(), catch_body.as_mut_slice()];
            if let Some(f) = finally_body {
                out.push(f.as_mut_slice());
            }
            out
        }
        Stmt::Switch { cases, default, .. } => {
            let mut out: Vec<&mut [Stmt]> =
                cases.iter_mut().map(|c| c.body.as_mut_slice()).collect();
            if let Some(d) = default {
                out.push(d.as_mut_slice());
            }
            out
        }
        _ => walk::stmt_children(s)
            .into_iter()
            .map(std::slice::from_mut)
            .collect(),
    }
}

/// Is `s` an expression statement whose whole expression is a
/// `super(…)` site, as `is_site` recognizes one?
pub(in crate::ast) fn binds(ast: &Ast, s: &Stmt, is_site: &dyn Fn(&Ast, ExprId) -> bool) -> bool {
    matches!(s, Stmt::Expr(e) if is_site(ast, *e))
}

/// The parser's `super(…)` node, before any rewrite has replaced it.
pub(in crate::ast) fn parser_site(ast: &Ast, e: ExprId) -> bool {
    matches!(ast.get_expr(e), Expr::Super { .. })
}

/// Call `f` on every statement of `list`, and of the lists nested in
/// it, that may run before the binding: a list stops after its first
/// binding statement. The binding statement itself is visited, since
/// its arguments run first.
pub(in crate::ast) fn each_unbound<'a>(
    ast: &Ast,
    list: &'a [Stmt],
    is_site: &dyn Fn(&Ast, ExprId) -> bool,
    f: &mut dyn FnMut(&'a Stmt),
) {
    for s in list {
        f(s);
        for l in lists(s) {
            each_unbound(ast, l, is_site, f);
        }
        if binds(ast, s, is_site) {
            break;
        }
    }
}

/// The expression roots of the statements [`each_unbound`] visits.
pub(in crate::ast) fn unbound_roots(
    ast: &Ast,
    list: &[Stmt],
    is_site: &dyn Fn(&Ast, ExprId) -> bool,
) -> Vec<ExprId> {
    let mut out = Vec::new();
    each_unbound(ast, list, is_site, &mut |s| out.extend(walk::stmt_exprs(s)));
    out
}
