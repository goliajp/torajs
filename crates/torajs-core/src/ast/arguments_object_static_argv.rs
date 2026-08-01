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
    let fwd_sites = forwarder_call_callee_eids(ast);
    let mut out: HashMap<String, usize> = HashMap::new();
    for name in cand {
        if fn_local_names.contains(&name) {
            continue;
        }
        if let Some(n) = uniform_direct_call_argc(ast, &name, 0, &fwd_sites) {
            out.insert(name, n);
        }
    }
    out
}

/// Shared reference analysis (knife 3a + method face): every arena
/// `Ident(name)` must be a direct Call callee, and all sites must
/// pass ONE uniform user arg count. `this_slots` leading args are
/// excluded from the count — 1 for `__cm_` methods (the pass-2
/// rewrite carries the receiver at arg[0]), 0 for plain named fns.
///
/// `fwd_sites` — Call callee eids inside synthesized `__forward_*`
/// bodies (see `forwarder_call_callee_eids`). A value escape
/// (`var ref = C.method`) reroutes through such a forwarder, whose
/// declared-arity relay call would otherwise break the uniform-argc
/// gate for every escaped fn. The relay is a direct call (legal),
/// but not a USER site: it stays out of the argc vote. Escape calls
/// through the forwarder pad missing trailing params (injected
/// extras included) with undefined — memory-safe, wrong-at-parity
/// with the pre-face FoldArity answer (test262
/// cls-*-meth-static-args-trailing-comma family stores the escape
/// without calling it).
fn uniform_direct_call_argc(
    ast: &Ast,
    name: &str,
    this_slots: usize,
    fwd_sites: &std::collections::HashSet<u32>,
) -> Option<usize> {
    use std::collections::HashSet;
    let eids: HashSet<u32> = (0..ast.exprs.len() as u32)
        .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == name))
        .collect();
    if eids.is_empty() {
        return None;
    }
    let mut argcs: HashSet<usize> = HashSet::new();
    let mut legal: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && eids.contains(&callee.0)
        {
            // A spread argument's runtime length is not statically
            // known — an args.len() count would under-report it and
            // the face would answer a wrong argc (silent). The fn
            // stays out of the static face (loud / Real tier).
            if args
                .iter()
                .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
            {
                return None;
            }
            legal.insert(callee.0);
            if !fwd_sites.contains(&callee.0) {
                argcs.insert(args.len().saturating_sub(this_slots));
            }
        }
    }
    if eids.difference(&legal).next().is_some() {
        return None;
    }
    if argcs.len() == 1 {
        argcs.into_iter().next()
    } else {
        None
    }
}

/// Collect the relay-call callee eids inside synthesized
/// `__forward_<target>` bodies (single `return target(args...)` —
/// see forwarders_object::synthesize_forwarder_decls).
fn forwarder_call_callee_eids(ast: &Ast) -> std::collections::HashSet<u32> {
    let mut out = std::collections::HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && name.starts_with("__forward_")
            && let [Stmt::Return(Some(eid))] = body.as_slice()
            && let Expr::Call { callee, .. } = ast.get_expr(*eid)
        {
            out.insert(callee.0);
        }
    }
    out
}

/// RFC 20260801-arguments-method-face knife 1 — single-owner class
/// methods join the static-argv face. The pass-2 desugar rewrote
/// every method call to `__cm_<C>__<M>(receiver, ...user)`, so the
/// same all-refs-direct + uniform-argc analysis applies with the
/// receiver slot excluded. Length-only bodies qualify too (methods
/// have no T-31 real-argc tier — their historical FoldArity answer
/// was the declared arity, silent-wrong on any other argc).
/// Member-read escapes (`var ref = C.prototype.m`) are invisible to
/// the Ident scan but memory-safe: boxed/escape calls pad missing
/// trailing params (the injected extras included) with undefined,
/// and their fold answers stay wrong-at-parity with FoldArity.
/// Multi-owner methods dispatch through `__dispatch_<M>` / raw
/// Member — excluded via the method_owners side table. Defaults /
/// rest params interleave with apply_args argc bookkeeping —
/// defensively excluded.
pub(super) fn collect_method_static_argv(ast: &Ast) -> std::collections::HashMap<String, usize> {
    use std::collections::HashMap;
    let fwd_sites = forwarder_call_callee_eids(ast);
    let mut out: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        if !name.starts_with("__cm_") {
            continue;
        }
        if params.first().is_none_or(|p| p.name != "__this") {
            continue;
        }
        if params.iter().any(|p| p.default.is_some() || p.is_rest) {
            continue;
        }
        if ast
            .method_owners
            .iter()
            .any(|(m, owners)| owners.iter().any(|o| name == &format!("__cm_{o}__{m}")))
        {
            continue;
        }
        if !body_has_non_length_arguments_touch(ast, body)
            && !super::arguments_object_walkers::body_has_arguments_length(ast, body)
        {
            continue;
        }
        if let Some(n) = uniform_direct_call_argc(ast, name, 1, &fwd_sites) {
            out.insert(name.clone(), n);
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
