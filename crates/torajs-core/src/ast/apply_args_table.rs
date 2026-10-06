//! The callee-side default tables the call-site passes read: which
//! leading params are hidden, and each FnDecl's paddable default list.
//! Moved out of `apply_args.rs` (file-size limit).

use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// Peel a FnDecl's hidden leading params, leaving the user-facing ones.
///
/// A lifted closure leads with `__env`. An object-literal method (RFC
/// 20260714-objlit-accessor blade 1) leads with `__env, __this` — the
/// Member-call path supplies the receiver, so no call site counts it and
/// neither may the default / rest tables.
///
/// A CLASS method (`__cm_C__M`) leads with a bare `__this` and NO
/// `__env`, and its consumer below peels that itself (`defaults[1..]`).
/// So `__this` is only hidden here when it sits BEHIND `__env` —
/// stripping a leading one too would peel it twice and eat a real user
/// param ("expected 2 argument(s), got 1").
pub(crate) fn peel_hidden_params(params: &[Param]) -> &[Param] {
    // RFC 20260816-headless-argv-face — a head-less argv body leads
    // with the synthetic raw-argv pointer instead of a head param.
    // The direct-call terminal fills that slot from its own buffer,
    // so no call site counts it and neither may the default / rest
    // tables (leaving it in shifted every default one position and
    // made `f()` on `function f(a = 5)` refuse the call).
    if params.first().is_some_and(|p| p.name == "__torajs_argv") {
        return &params[1..];
    }
    if !params.first().is_some_and(|p| p.name == "__env") {
        return params;
    }
    let rest = &params[1..];
    if rest.first().is_some_and(|p| p.name == "__this") {
        return &rest[1..];
    }
    rest
}

/// Map every FnDecl with at least one defaulted param to its
/// user-param default list (hidden leading params peeled).
/// Shared between `apply_default_args` (call-site padding) and
/// `apply_spread_args` (defaulted-slot ternary expansion).
pub(crate) fn collect_fn_defaults(ast: &Ast) -> HashMap<String, Vec<Option<ExprId>>> {
    let mut fn_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let mut user_params: &[Param] = peel_hidden_params(params);
            // A closure on the argv face leads its user params with the
            // synthetic argv pointer, and every call reaches it through
            // the boxed entry, which reads a missing slot as undefined.
            // Padding it with `undefined` (a moved default's pad) would
            // only change `arguments.length`; a default still evaluated
            // at the call site keeps its pad, one slot past the pointer.
            if let Some((head, rest)) = user_params.split_first()
                && head.name == "__torajs_argv"
            {
                if rest
                    .iter()
                    .all(|p| p.default.is_none_or(|d| is_undefined(ast, d)))
                {
                    continue;
                }
                user_params = rest;
            }
            if user_params.iter().any(|p| p.default.is_some()) {
                fn_defaults.insert(name.clone(), paddable_defaults(user_params));
            }
        }
    }
    fn_defaults
}

fn is_undefined(ast: &Ast, d: ExprId) -> bool {
    matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined")
}

/// The paddable slots of a user-param list: the fixed prefix, with a
/// trailing rest position dropped.
///
/// A rest position is not an omitted argument the caller can supply.
/// What §10.2.11 owes an absent variadic tail is an EMPTY array, so
/// an `undefined` padded there lands IN the tail instead — `g()` on
/// `g(x = 5, ...r)` would answer `r.length` 1. The tail is the
/// callee's own; padding stops in front of it.
///
/// `apply_spread_args`, this table's other reader, indexes it up to
/// a separately computed arity — it never reaches past this, because
/// it skips variadic callees outright (`apply_rest_args` consumed
/// their spread sites already).
fn paddable_defaults(user_params: &[Param]) -> Vec<Option<ExprId>> {
    let fixed = match user_params.last() {
        Some(p) if p.is_rest => &user_params[..user_params.len() - 1],
        _ => user_params,
    };
    fixed.iter().map(|p| p.default).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, is_rest: bool) -> Param {
        Param {
            name: name.into(),
            type_ann: None,
            default: None,
            is_rest,
        }
    }

    fn pd(name: &str, default: Option<ExprId>) -> Param {
        Param {
            name: name.into(),
            type_ann: None,
            default,
            is_rest: false,
        }
    }

    #[test]
    fn the_rest_position_is_not_a_paddable_slot() {
        let ps = vec![pd("x", Some(ExprId(0))), p("r", true)];
        assert_eq!(paddable_defaults(&ps), vec![Some(ExprId(0))]);
    }

    #[test]
    fn a_fixed_tail_keeps_every_slot() {
        let ps = vec![p("x", false), pd("y", Some(ExprId(0)))];
        assert_eq!(paddable_defaults(&ps), vec![None, Some(ExprId(0))]);
    }
}
