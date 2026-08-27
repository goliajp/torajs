//! The two recursive walks over one default EXPRESSION — extracted
//! when the pad-reachability admission pushed the parent past the
//! file-size line.
//!
//! The seam is which half of the question each side answers. Its
//! sibling gates in the parent (`converting_default` /
//! `converting_typed_default`) read the PARAM — its annotation, its
//! position — to pick a lane. These two read only the expression
//! that was written after the `=`, and answer what it allows: may it
//! be evaluated inside the callee at all, and does it read a name
//! that only the callee's own scope binds.

use super::*;
use crate::ast::{Param, free_vars};

/// Whether the default is safe to evaluate inside the callee body —
/// a recursive WHITELIST over expression shapes. A function literal
/// converts when its free vars are all PRIOR PARAMS of the owning fn
/// (`h2(k, cb = (x) => x + k)` — moved into the body, the arrow
/// captures the param like any body closure, and the params are
/// bound before the guard runs) or when it is CLOSED (free-var set
/// empty after the top-level-FnDecl / builtin filter — the t262
/// abrupt template `(function() { throw new Test262Error(); }())`),
/// which is what lets an async fn's throwing default reject instead
/// of throwing synchronously (see [`super::splice_guards`]). Any OTHER
/// capture (the IIFE shape `_ = (() => { cc++; })()` reading a
/// top-level `let`) stays on the call-site pad: a closure minted
/// inside a fn body cannot capture a top-level `let` today
/// (pre-existing "closure capture not in scope", gate-caught on the
/// first cut of this pass), while the padded copy sits at the
/// top-level call site where the capture works. A LATER-param
/// capture also stays on the pad — its param may itself convert to a
/// body `let` declared after this guard, and the capture passes owe
/// no forward-reference story. Unknown shapes answer false — keeping
/// today's behavior is always safe.
pub(super) fn body_safe_default(
    ast: &Ast,
    e: ExprId,
    global_fns: &[String],
    prior: &[String],
) -> bool {
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null | Expr::Ident(_) => true,
        Expr::BinOp { left, right, .. } => {
            body_safe_default(ast, *left, global_fns, prior)
                && body_safe_default(ast, *right, global_fns, prior)
        }
        Expr::Unary { expr, .. } | Expr::As { expr, .. } => {
            body_safe_default(ast, *expr, global_fns, prior)
        }
        Expr::Member { obj, .. } => body_safe_default(ast, *obj, global_fns, prior),
        Expr::Index { obj, index } => {
            body_safe_default(ast, *obj, global_fns, prior)
                && body_safe_default(ast, *index, global_fns, prior)
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            body_safe_default(ast, *cond, global_fns, prior)
                && body_safe_default(ast, *then_branch, global_fns, prior)
                && body_safe_default(ast, *else_branch, global_fns, prior)
        }
        Expr::Sequence { left, right } => {
            body_safe_default(ast, *left, global_fns, prior)
                && body_safe_default(ast, *right, global_fns, prior)
        }
        Expr::Call { callee, args } => {
            body_safe_default(ast, *callee, global_fns, prior)
                && args
                    .iter()
                    .all(|a| body_safe_default(ast, *a, global_fns, prior))
        }
        Expr::Array(elems) => elems
            .iter()
            .all(|a| body_safe_default(ast, *a, global_fns, prior)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .all(|(_, v)| body_safe_default(ast, *v, global_fns, prior)),
        Expr::ArrowFn { params, body, .. } => {
            free_vars::free_vars_of_arrow(ast, params, body, global_fns, None)
                .iter()
                .all(|v| prior.contains(v))
        }
        _ => false,
    }
}

/// Whether the default expression reads a STRICTLY-PRIOR param name —
/// the one FnDecl shape whose call-site pad is unsound (§9.2 wants
/// the reference bound in the CALLEE's params environment; pasting it
/// at the call site read the caller's scope: `unknown identifier` +
/// runtime ReferenceError, the L3b ④ recon shape). Self/later bare
/// reads were already rewritten to throw IIFEs by
/// `desugar_dflt_param_tdz` (they carry no param reference and stay
/// on the pad); compound self/later reads are NOT prior refs and stay
/// out too — converting one would put the read ahead of its own
/// `let`, trading a runtime TDZ for a checker reject. Only walks the
/// shapes [`body_safe_default`] admits; an ArrowFn references a
/// prior param through its FREE-VAR set (the capture, not a direct
/// read).
pub(super) fn refs_prior_param(
    ast: &Ast,
    params: &[Param],
    pi: usize,
    d: ExprId,
    global_fns: &[String],
) -> bool {
    let rec = |a| refs_prior_param(ast, params, pi, a, global_fns);
    match ast.get_expr(d) {
        Expr::Ident(n) => params[..pi].iter().any(|p| p.name == *n),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            rec(*left) || rec(*right)
        }
        Expr::Unary { expr, .. } | Expr::As { expr, .. } => rec(*expr),
        Expr::Member { obj, .. } => rec(*obj),
        Expr::Index { obj, index } => rec(*obj) || rec(*index),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => rec(*cond) || rec(*then_branch) || rec(*else_branch),
        Expr::Call { callee, args } => rec(*callee) || args.iter().any(|a| rec(*a)),
        Expr::Array(elems) => elems.iter().any(|a| rec(*a)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, v)| rec(*v)),
        Expr::ArrowFn {
            params: aparams,
            body,
            ..
        } => free_vars::free_vars_of_arrow(ast, aparams, body, global_fns, None)
            .iter()
            .any(|v| params[..pi].iter().any(|p| p.name == *v)),
        _ => false,
    }
}
