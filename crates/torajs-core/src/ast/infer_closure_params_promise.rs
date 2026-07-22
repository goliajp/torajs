//! Promise `.then(cb)` / `.catch(cb)` cb-param inference — the
//! sibling arm of [`crate::ast::infer_closure_params`], extracted so
//! the main call-site loop stays within the file-size hard limit.
//!
//! The narrow contract: given a `.then` / `.catch` receiver
//! expression, walk two shapes back to the source Promise's inner
//! type so the callback's un-annotated user param can be seeded
//! before `desugar_lifted_closure_fn` defaults it to `any`:
//!
//! - `Promise.resolve(v)` / `Promise.reject(v)` mints — the inner is
//!   `v`'s static type (literal or annotated ident).
//! - Chained `.then(prev_cb)` / `.catch(prev_cb)` — the inner is
//!   `prev_cb`'s return annotation. Explicit or already-sniffable
//!   returns come from `fn_ret_anns` (populated once up front from
//!   the pre-mutation FnDecls). When `prev_cb`'s body sniff failed
//!   because a user param was un-annotated (`n => n`), fall through
//!   to a recursive resolve of the receiver's chain, seed the param,
//!   and re-sniff — the second try then succeeds and the ret ann
//!   propagates. Recursion is bounded by the AST shape.

use super::infer_closure_params::infer_lit_ann;
use super::{Ast, Expr, ExprId, Stmt, infer_return_ann};

pub(super) fn resolve_promise_inner_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
    fn_ret_anns: &std::collections::HashMap<String, Option<String>>,
) -> Option<String> {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    match name.as_str() {
        "resolve" | "reject" => {
            if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Promise") {
                return None;
            }
            args.first()
                .and_then(|a| resolve_arg_ann(ast, *a, all_anns))
        }
        "then" | "catch" => {
            let cb = *args.first()?;
            let fn_name = match ast.get_expr(cb) {
                Expr::Closure { fn_name, .. } => fn_name.clone(),
                Expr::Ident(n) if n.starts_with("__closure_") => n.clone(),
                _ => return None,
            };
            if let Some(Some(r)) = fn_ret_anns.get(&fn_name) {
                return Some(r.clone());
            }
            // prev_cb's un-annotated user param blocked the up-front
            // sniff — seed it from the receiver's inner and try
            // again. Identity chains (`n => n`) propagate through
            // here without needing a mid-pass rebuild.
            let prev_inner = resolve_promise_inner_ann(ast, *obj, all_anns, fn_ret_anns)?;
            sniff_body_with_param_seed(ast, &fn_name, &prev_inner)
        }
        _ => None,
    }
}

/// Rerun the return-ann sniff for a lifted closure body with the
/// first user param seeded to `param_inner_ann` — used when the
/// up-front sniff failed only because the param carried no ann.
fn sniff_body_with_param_seed(ast: &Ast, fn_name: &str, param_inner_ann: &str) -> Option<String> {
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
            && name == fn_name
        {
            let mut synth = params.clone();
            let user_start = if synth.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            if let Some(p) = synth.get_mut(user_start)
                && p.type_ann.is_none()
            {
                p.type_ann = Some(param_inner_ann.to_string());
            }
            return infer_return_ann(&ast.exprs, body, &synth, &std::collections::HashMap::new());
        }
    }
    None
}

/// Best-effort type annotation for an expression flowing into a
/// `Promise.resolve/reject(arg)` mint — the arg's static shape.
fn resolve_arg_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Ident(n) => all_anns.get(n).cloned(),
        _ => infer_lit_ann(ast, eid),
    }
}
