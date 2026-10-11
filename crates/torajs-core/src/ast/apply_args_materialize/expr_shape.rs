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
use crate::ast::desugar_with::walk::expr_children;
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
///
/// `in_fn_decl`: an assignment default (`n += 1`, `count++` — the
/// t262 dflt-params-arg-val templates) writes a top-level binding,
/// which a named fn body does through the global lane; moved into an
/// arrow body it would be a closure capture instead, so only the
/// FnDecl lane admits it.
pub(super) fn body_safe_default(
    ast: &Ast,
    e: ExprId,
    global_fns: &[String],
    prior: &[String],
    in_fn_decl: bool,
) -> bool {
    let rec = |a| body_safe_default(ast, a, global_fns, prior, in_fn_decl);
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null | Expr::Ident(_) => true,
        Expr::Assign { target, value } => in_fn_decl && rec(*target) && rec(*value),
        Expr::PostIncr { target, .. } => in_fn_decl && rec(*target),
        Expr::ArrowFn { params, body, .. } => {
            free_vars::free_vars_of_arrow(ast, params, body, global_fns, &[], None)
                .iter()
                .all(|v| prior.contains(v))
        }
        other if composite(other) => expr_children(ast, e).into_iter().all(rec),
        _ => false,
    }
}

/// The shapes whose parts alone decide: an operator, a member or index
/// read, a call or construction, a `typeof`, an optional chain, `??`,
/// `instanceof`, a spread, and the literals that build a fresh value.
fn composite(e: &Expr) -> bool {
    matches!(
        e,
        Expr::BinOp { .. }
            | Expr::Unary { .. }
            | Expr::As { .. }
            | Expr::Member { .. }
            | Expr::Index { .. }
            | Expr::Ternary { .. }
            | Expr::Sequence { .. }
            | Expr::Call { .. }
            | Expr::Array(_)
            | Expr::ObjectLit { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::TypeOf { .. }
            | Expr::OptChain { .. }
            | Expr::OptIndex { .. }
            | Expr::OptCall { .. }
            | Expr::Nullish { .. }
            | Expr::InstanceOf { .. }
            | Expr::Spread { .. }
            | Expr::BigInt { .. }
            | Expr::Regex { .. }
    )
}

/// Whether the default expression reads `arguments` outside any nested
/// function literal. In a function expression's parameter list that
/// is the function's own arguments object (§10.2.11 steps 18-22 bind
/// it before the parameters are evaluated), which the call site
/// cannot name — so such a default has to move into the body.
pub(super) fn reads_arguments(ast: &Ast, d: ExprId) -> bool {
    let rec = |a| reads_arguments(ast, a);
    match ast.get_expr(d) {
        Expr::Ident(n) => n == "arguments",
        other
            if composite(other) || matches!(other, Expr::Assign { .. } | Expr::PostIncr { .. }) =>
        {
            expr_children(ast, d).into_iter().any(rec)
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
        Expr::ArrowFn {
            params: aparams,
            body,
            ..
        } => free_vars::free_vars_of_arrow(ast, aparams, body, global_fns, &[], None)
            .iter()
            .any(|v| params[..pi].iter().any(|p| p.name == *v)),
        other
            if composite(other) || matches!(other, Expr::Assign { .. } | Expr::PostIncr { .. }) =>
        {
            expr_children(ast, d).into_iter().any(rec)
        }
        _ => false,
    }
}
