//! Named-fn HOF callback `thisArg` — the receiver-first forwarder.
//!
//! `[11].every(callbackfn, objArray)` where `callbackfn` is a NAMED
//! fn declaration whose body says `this`: `bind_this_param` gave the
//! decl a hidden `__this: any` first param, and the fn-to-closure
//! collector's cluster-#1 axis wraps its value use in the plain
//! `__forward_<name>` shim — which hardwires `undefined` into
//! `__this` (plain-call receiver semantics). ES §23.1.3.{6,8-11,30}
//! say the callback's `this` is the thisArg T, so every such case
//! ran the callback with `this === undefined` (the rotation-284
//! re-baseline exposed 22 every/some cases riding exactly this).
//!
//! Fix: at a receiver-aware HOF callback position WITH a thisArg
//! present, the value use gets a RECEIVER-FIRST forwarder instead —
//!
//! ```text
//! __fwdrecv_<name>(__env, __this: any, ...user) {
//!     return <name>(__this, ...user)
//! }
//! ```
//!
//! — registered in `ast.fnexpr_recv_fns`, so the whole rotation-261
//! knife-4 protocol picks it up unchanged: the closure mint stamps
//! `FLAG_CLOSURE_RECV_FIRST`, `closure_value_ty` /
//! `check_type_of_fn::closure_sig` shed `__this` from the VALUE
//! type, the typed-array trio + predicate lowerings thread the boxed
//! thisArg as the leading argv entry (`sig_skip`), and the any-lane
//! runtime kernels seed argv[0] off the header flag.
//!
//! The zero-alias bar is met by construction: only the callback SLOT
//! rewrites (a fresh `Expr::Closure` per site, consumed by that HOF
//! call alone). The named fn itself keeps its normal ABI everywhere
//! else — direct calls, plain `__forward_` value uses, and
//! thisArg-less callback positions are untouched.
//!
//! Receiver gates mirror `fnexpr_this`'s knife-4 exactly (a
//! promoted closure must only reach receiver-aware call paths — a
//! struct-field or sibling-collision method named `every` would call
//! it with shifted args): inline array literals, syntactically-
//! certain array-literal bindings, `: any` / dynobj-certain
//! bindings, and Map/Set bindings for `forEach`. Shadowed or
//! re-declared callback names, rest-param targets, and generator-
//! family fns keep today's plain-forwarder route (over-refusal =
//! the old undefined-`this`, never a mis-promote).
//!
//! Pipeline position: BEFORE `synthesize_fn_to_closure_forwarders` —
//! the rewrite turns the site's Ident into `Expr::Closure`, which
//! cluster #1's Ident-only walk then naturally skips (no double
//! wrap). After `bind_this_param` (reads the `__this` param it
//! inserts) and after `desugar_nested_fns` (lifted nested names are
//! top-level by then).

use super::fnexpr_this::is_hof_method;
use super::fnexpr_this_recvs::{
    collect_any_binding_names, collect_arraylit_binding_names, collect_decls_by_name,
    collect_mapset_binding_names, name_shadowed_elsewhere,
};
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

pub fn synthesize_recv_cb_forwarders(ast: &mut Ast) {
    let fn_sigs = collect_promoted_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return;
    }
    let sites = collect_sites(ast, &fn_sigs);
    if sites.is_empty() {
        return;
    }
    let mut renames: HashMap<String, String> = HashMap::new();
    for (_, target) in &sites {
        if renames.contains_key(target) {
            continue;
        }
        let fwd = synthesize_one(ast, target, &fn_sigs);
        ast.fnexpr_recv_fns.insert(fwd.clone());
        renames.insert(target.clone(), fwd);
    }
    for (eid, target) in sites {
        ast.exprs[eid.0 as usize] = Expr::Closure {
            fn_name: renames[&target].clone(),
            captures: Vec::new(),
        };
    }
}

/// Top-level plain fns `bind_this_param` promoted (`__this` first
/// param). Synthetic names (`__cm_` methods, forwarders, lifted
/// closures) never qualify — the `__` prefix is not user-spellable.
/// Rest-param targets stay out: the recv forwarder forwards by
/// positional name, which a variadic tail can't ride.
fn collect_promoted_fn_sigs(
    ast: &Ast,
) -> HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> {
    let mut out = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
            && !name.starts_with("__")
            && params.first().is_some_and(|p| p.name == "__this")
            && !params.iter().any(|p| p.is_rest)
            && !params.iter().any(|p| p.name == crate::ast::GEN_ARGV_PARAM)
        {
            out.insert(name.clone(), (params.clone(), return_type.clone(), *span));
        }
    }
    out
}

/// Every `(cb ExprId, target name)` where a promoted named fn sits
/// in a receiver-aware HOF callback slot with a thisArg present.
fn collect_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
) -> Vec<(ExprId, String)> {
    let any_recvs = collect_any_binding_names(&ast.stmts, &ast.exprs);
    let arraylit_recvs = collect_arraylit_binding_names(&ast.stmts, &ast.exprs);
    let mapset_recvs = collect_mapset_binding_names(&ast.stmts, &ast.exprs);
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name: mname } = ast.get_expr(*callee) else {
            continue;
        };
        // `reduce`/`reduceRight` take no thisArg (§23.1.3.24) — the
        // allowlist already excludes them.
        if !is_hof_method(mname) || args.len() < 2 {
            continue;
        }
        let cb = args[0];
        let Expr::Ident(cbname) = ast.get_expr(cb) else {
            continue;
        };
        if !fn_sigs.contains_key(cbname)
            || ast.generator_factory_classes.contains_key(cbname)
            || ast.async_generator_fns.contains(cbname)
        {
            continue;
        }
        // Receiver-aware gate — knife-4 mirror (see module doc).
        let typed_ok = mname != "flatMap";
        let mapset_ok = mname == "forEach";
        let recv_ok = (typed_ok && matches!(ast.get_expr(*obj), Expr::Array(_)))
            || matches!(ast.get_expr(*obj), Expr::Ident(n)
                if any_recvs.contains(n)
                    || (typed_ok && arraylit_recvs.contains(n))
                    || (mapset_ok && mapset_recvs.contains(n)));
        if !recv_ok {
            continue;
        }
        // A shadowed or re-declared name cannot be paired to the top
        // decl syntactically — keep the plain-forwarder route.
        if name_shadowed_elsewhere(&ast.stmts, cbname) {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        collect_decls_by_name(&ast.stmts, cbname, &mut decls);
        if !decls.is_empty() {
            continue;
        }
        sites.push((cb, cbname.clone()));
    }
    sites
}

/// Mint `__fwdrecv_<target>(__env, __this, ...user) { return
/// target(__this, ...user) }` and append the decl. The target's own
/// params already lead with `__this: any`, so the forwarder's face
/// is `__env` + the target params verbatim, and the forwarding call
/// passes every one through by name.
fn synthesize_one(
    ast: &mut Ast,
    target: &str,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
) -> String {
    let fwd = format!("__fwdrecv_{target}");
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if *name == fwd))
    {
        return fwd;
    }
    let (params, return_type, target_span) = fn_sigs[target].clone();
    let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
    fwd_params.push(Param {
        name: "__env".into(),
        type_ann: Some("__env()".to_string()),
        default: None,
        is_rest: false,
    });
    fwd_params.extend(params.iter().cloned());
    let mut arg_eids: Vec<ExprId> = Vec::with_capacity(params.len());
    for p in &params {
        arg_eids.push(ast.add_expr(Expr::Ident(p.name.clone())));
    }
    let callee_id = ast.add_expr(Expr::Ident(target.to_string()));
    let call_id = ast.add_expr(Expr::Call {
        callee: callee_id,
        args: arg_eids,
    });
    ast.stmts.push(Stmt::FnDecl {
        name: fwd.clone(),
        type_params: Vec::new(),
        params: fwd_params,
        return_type,
        body: vec![Stmt::Return(Some(call_id))],
        is_generator: false,
        // The target's source span — toString answers the wrapped
        // user fn's source (B4, same as the plain forwarder).
        span: target_span,
    });
    fwd
}
