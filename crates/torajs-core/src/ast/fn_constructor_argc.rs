//! Variadic forwarding for the `__fnctor_` factories.
//!
//! The factory [`super::fn_constructor`] mints forwards by naming each
//! declared param, which silently drops every surplus `new F(…)`
//! argument — `function H() { this.n = arguments.length; } new H(1,2,3)`
//! answered 0: the checker admits the over-arity factory call, the
//! lowerer drops the extras, and the arguments face only ever sees the
//! factory's own 0-arg direct call to `H`.
//!
//! Fix shape: when every construct site of a name passes the SAME
//! argument count (the test262 constructor cases are single-site, so
//! trivially uniform) and the constructed body actually touches
//! `arguments`, the factory grows trailing `__ctor_extra_*: any`
//! params and forwards all of them. The callee's single direct call —
//! inside the factory — then carries the true argc, and the existing
//! static-argv face (`arguments_object_static_argv`, knife 3a)
//! injects the matching trailing params on the callee so the body's
//! `arguments` answers the construct site's truth.
//!
//! A body with no `arguments` touch keeps the declared-arity factory:
//! surplus args stay checker-admitted and dropped, which is exactly
//! the ES semantics for a constructor that never looks at them —
//! growing the factory there would be pure blast radius.
//!
//! Sites that disagree on argc get `None` and keep today's shape; a
//! spread argument's length is not static, so it poisons the name the
//! same way (mirroring the face's own spread rule).

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::fn_constructor::Constructible;
use super::{Ast, Expr, Param, Stmt};
use std::collections::HashMap;

/// Does this body read `arguments` in any form (length included)?
/// Fills [`Constructible::body_touches_arguments`] at collect time.
pub(super) fn body_touches_arguments(ast: &Ast, body: &[Stmt]) -> bool {
    body_has_arguments_length(ast, body) || body_has_non_length_arguments_touch(ast, body)
}

/// Per-name uniform construct-site argc: `Some(n)` when every
/// `__new_<name>(…)` call passes exactly `n` args and none of them is
/// a spread; `None` when sites disagree or a spread hides the count.
pub(super) fn uniform_construct_argc(ast: &Ast) -> HashMap<String, Option<usize>> {
    let mut out: HashMap<String, Option<usize>> = HashMap::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(n) = ast.get_expr(*callee) else {
            continue;
        };
        let Some(bare) = n.strip_prefix("__new_") else {
            continue;
        };
        let this_len = if args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            None
        } else {
            Some(args.len())
        };
        out.entry(bare.to_string())
            .and_modify(|prev| {
                if *prev != this_len {
                    *prev = None;
                }
            })
            .or_insert(this_len);
    }
    out
}

/// The factory's param list for one constructible: the declared user
/// params — reshaped to EXACTLY the uniform construct-site argc when
/// the body reads `arguments`, so the callee's direct call carries
/// the true argument count in both directions:
///
/// - surplus sites grow trailing `__ctor_extra_<name>_<i>: any`
///   params (the name embeds the constructible — the
///   `__torajs_iife_extra_` precedent: a bare index collides across
///   fns in any name-keyed downstream analysis);
/// - under-filled sites TRUNCATE to the passed count. Forwarding the
///   full declared list would let the checker's T-28 pad fill the
///   factory call's missing tail with undefined, and the face would
///   count those pads into `arguments.length` (probe: `new S(7, 8)`
///   on a 3-param S answered 3). The callee's own missing params
///   still pad to undefined at ITS call site — the ES answer.
pub(super) fn factory_params(c: &Constructible, site_argc: Option<&Option<usize>>) -> Vec<Param> {
    if !c.body_touches_arguments {
        return c.params.clone();
    }
    let Some(&Some(n)) = site_argc else {
        return c.params.clone();
    };
    let mut params: Vec<Param> = c.params.iter().take(n).cloned().collect();
    for i in 0..n.saturating_sub(c.params.len()) {
        params.push(Param {
            name: format!("__ctor_extra_{}_{}", c.name, i),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
    }
    params
}
