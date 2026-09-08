//! The injection half of the static-argv face — how a settled
//! static argv count turns into actual parameter slots.
//!
//! Its siblings answer WHO is on the face and WHAT the count is;
//! this module answers what to do with that answer: grow each
//! admitted fn by the trailing `__torajs_iife_extra_*` slots its
//! largest call site needs, and grow the class factory by the same
//! slots so the surplus a `new` site wrote actually reaches the
//! constructor it was written for.
//!
//! Split from `arguments_object_static_argv` in rotation 616, which
//! had grown to 659 lines.

use super::{Ast, Expr, Param, Stmt};

/// The factory half of the constructor face. `inject_iife_static_params`
/// gives the ctor its trailing `__torajs_iife_extra_*` slots, but the
/// only site that calls a ctor is its own factory — whose parameter
/// list is the ctor's DECLARED params, so the surplus the `new` site
/// wrote died there and the injected slots had nothing feeding them.
///
/// Growing the factory by the same slots and forwarding them at the
/// inner ctor call closes the channel end to end: the `new` site
/// already evaluates every argument the program wrote (the terminal
/// lowers them all and aligns down), so widening the factory is
/// enough for them to arrive.
pub(super) fn inject_ctor_factory_extras(
    ast: &mut Ast,
    face: &std::collections::HashMap<String, usize>,
) {
    let mut work: Vec<(String, String, Vec<String>)> = Vec::new();
    for ctor in face.keys() {
        let Some(cname) = ctor
            .strip_prefix("__cm_")
            .and_then(|r| r.strip_suffix("__ctor"))
        else {
            continue;
        };
        let prefix = format!("__torajs_iife_extra_{ctor}_");
        let extras: Vec<String> = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { name, params, .. } if name == ctor => Some(
                    params
                        .iter()
                        .filter(|p| p.name.starts_with(&prefix))
                        .map(|p| p.name.clone())
                        .collect::<Vec<String>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        if extras.is_empty() {
            continue;
        }
        work.push((format!("__new_{cname}"), ctor.clone(), extras));
    }
    for (factory, ctor, extras) in work {
        // Only the FACTORY's own call forwards them. A `super(…)`
        // site is a direct ctor call too, and it already wrote the
        // arguments itself — they land in these same slots
        // positionally. Forwarding there would spell a name that
        // exists only inside the factory.
        let mut inner = vec![false; ast.exprs.len()];
        for s in &ast.stmts {
            if let Stmt::FnDecl { name, body, .. } = s
                && *name == factory
            {
                super::desugar_eval::body_owned_exprs(ast, body, &mut inner);
            }
        }
        let sites: Vec<usize> = ast
            .exprs
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                inner[*i]
                    && matches!(e, Expr::Call { callee, .. }
                        if matches!(ast.get_expr(*callee), Expr::Ident(n) if *n == ctor))
            })
            .map(|(i, _)| i)
            .collect();
        for i in sites {
            let ids: Vec<super::ExprId> = extras
                .iter()
                .map(|e| ast.add_expr(Expr::Ident(e.clone())))
                .collect();
            if let Expr::Call { args, .. } = &mut ast.exprs[i] {
                args.extend(ids);
            }
        }
        for s in ast.stmts.iter_mut() {
            if let Stmt::FnDecl { name, params, .. } = s
                && *name == factory
            {
                for e in &extras {
                    params.push(Param {
                        name: e.clone(),
                        type_ann: Some("any".into()),
                        default: None,
                        is_rest: false,
                    });
                }
            }
        }
    }
}

pub(super) fn inject_iife_static_params(
    ast: &mut Ast,
    iife_static_argv: &std::collections::HashMap<String, usize>,
    fn_params: &mut std::collections::HashMap<String, Vec<String>>,
) {
    if iife_static_argv.is_empty() {
        return;
    }
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, params, .. } = s
            && let Some(&n) = iife_static_argv.get(name)
        {
            // A trailing rest param already captures every over-arity
            // arg (apply_rest_args bundles them downstream) — a
            // positional extra would land AFTER the rest param,
            // breaking rest-must-be-last AND the bundling's
            // last-param probe. The materializer spreads the rest
            // array instead (see synth_arguments_local_rest).
            if params.last().is_some_and(|p| p.is_rest) {
                continue;
            }
            let Some(user_params) = fn_params.get_mut(name) else {
                continue;
            };
            // Name embeds the (unique) lifted-closure fn name: a
            // plain `__torajs_iife_extra_{i}` collides ACROSS
            // closures in any name-keyed downstream analysis.
            for i in 0..n.saturating_sub(user_params.len()) {
                let extra = format!("__torajs_iife_extra_{name}_{i}");
                params.push(Param {
                    name: extra.clone(),
                    type_ann: Some("any".into()),
                    default: None,
                    is_rest: false,
                });
                user_params.push(extra);
            }
        }
    }
}
