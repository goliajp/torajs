//! RFC 20260801-arguments-escape-face — the static-argv face
//! collectors + param injector for
//! [`super::arguments_object::desugar_arguments_object`], split to a
//! sibling when knife 3a pushed the main pass file past the 500-line
//! limit. Knife 1: an IIFE's single call site makes the whole argv
//! static. Knife 3a: a top-level named fn whose every reference is a
//! direct call with one uniform argc joins the same face.

use super::arguments_object_walkers::body_has_non_length_arguments_touch;
use super::{Ast, Expr, Param, Stmt};

/// RFC 20260801-arguments-escape-face knife 1 — collect IIFEs whose
/// body touches `arguments` in any non-length form. The single call
/// site makes the whole argv static: `fn name → call-site arg count`.
/// A fn spotted at two call sites is skipped defensively (lifted
/// IIFE closures have exactly one; two means this isn't the shape we
/// think it is — over-kill only loses the admit, never wrong).
pub(super) fn collect_iife_static_argv(
    ast: &Ast,
    iife_real_argc: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};
    let mut argc: HashMap<String, usize> = HashMap::new();
    let mut dup: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        if argc.insert(fn_name.clone(), args.len()).is_some() {
            dup.insert(fn_name.clone());
        }
    }
    for d in &dup {
        argc.remove(d);
    }
    argc.retain(|name, _| {
        !iife_real_argc.contains(name)
            && ast.stmts.iter().any(|s| {
                matches!(s, Stmt::FnDecl { name: n, body, .. }
                    if n == name && body_has_non_length_arguments_touch(ast, body))
            })
    });
    argc
}

/// RFC 20260801 knife 3a — top-level named fns join the static-argv
/// face when their argv is provably uniform: the body touches
/// `arguments` in a non-length form, no fn-local binding shadows the
/// name anywhere (conservative — same guard as the argc tier's HOF
/// track), every arena reference is a direct Call callee (a value
/// escape reaches call sites that know nothing about the injected
/// extras), and all sites pass the SAME arg count. Sites with
/// differing counts need the runtime-truncate tier (knife 3b,
/// recorded in the RFC) and stay loud for now.
pub(super) fn collect_named_static_argv(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};
    let mut cand: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
            && !env_fns.contains(name)
            && params.first().is_none_or(|p| p.name != "__this")
            && body_has_non_length_arguments_touch(ast, body)
        {
            cand.insert(name.clone());
        }
    }
    if cand.is_empty() {
        return HashMap::new();
    }
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                fn_local_names.insert(p.name.clone());
            }
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    let mut out: HashMap<String, usize> = HashMap::new();
    for name in cand {
        if fn_local_names.contains(&name) {
            continue;
        }
        let eids: HashSet<u32> = (0..ast.exprs.len() as u32)
            .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if *n == name))
            .collect();
        if eids.is_empty() {
            continue;
        }
        let mut argcs: HashSet<usize> = HashSet::new();
        let mut legal: HashSet<u32> = HashSet::new();
        for e in &ast.exprs {
            if let Expr::Call { callee, args } = e
                && eids.contains(&callee.0)
            {
                legal.insert(callee.0);
                argcs.insert(args.len());
            }
        }
        if eids.difference(&legal).next().is_some() {
            continue;
        }
        if argcs.len() == 1 {
            out.insert(name, argcs.into_iter().next().unwrap());
        }
    }
    out
}

/// RFC 20260801 knife 1 — append `__torajs_iife_extra_i: any` params
/// to each static-argv IIFE whose call site passes more args than
/// declared, so the over-arity values reach the body (instead of the
/// direct-call trailing-ignore drop). The rewrite param table gets
/// the same names so `arguments[<literal>]` and the materialized
/// array resolve them like any declared param.
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
