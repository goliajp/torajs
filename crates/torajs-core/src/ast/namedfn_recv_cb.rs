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
//! The callback slot admits the bare Ident and its `cb as any`
//! spelling (398-10) — the cast erases nothing at runtime, and the
//! typed-receiver route for that spelling is the any-lane wedge
//! (`check_type_of_call_arr_hof_any_cb`), whose kernels honor the
//! same header flag.
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
//! Which arena slots ask for the shell is a separate question with a
//! separate answer per position; those censuses live in the sibling
//! [`super::namedfn_recv_cb_sites`]. This module owns what the shell
//! IS and how it is minted.
//!
//! Pipeline position: BEFORE `synthesize_fn_to_closure_forwarders` —
//! the rewrite turns the site's Ident into `Expr::Closure`, which
//! cluster #1's Ident-only walk then naturally skips (no double
//! wrap). After `bind_this_param` (reads the `__this` param it
//! inserts) and after `desugar_nested_fns` (lifted nested names are
//! top-level by then).

use super::namedfn_recv_cb_sites::{
    collect_any_let_init_sites, collect_as_any_sites, collect_json_cb_sites,
    collect_objlit_field_sites, collect_props_push_sites, collect_props_store_sites, collect_sites,
};
use super::{Ast, Expr, Param, Stmt};
use std::collections::HashMap;

pub fn synthesize_recv_cb_forwarders(ast: &mut Ast) {
    let fn_sigs = collect_promoted_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return;
    }
    let mut sites = collect_sites(ast, &fn_sigs);
    collect_objlit_field_sites(ast, &fn_sigs, &mut sites);
    collect_any_let_init_sites(ast, &fn_sigs, &mut sites);
    collect_props_store_sites(ast, &fn_sigs, &mut sites);
    collect_props_push_sites(ast, &fn_sigs, &mut sites);
    collect_json_cb_sites(ast, &fn_sigs, &mut sites);
    collect_as_any_sites(ast, &fn_sigs, &mut sites);
    super::namedfn_recv_cb_from::collect_array_from_sites(ast, &fn_sigs, &mut sites);
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

/// The arena slots this pass will claim, for a pass that runs
/// EARLIER and must not take them first. `promote_this_fn_values`
/// gives every other value use of a promoted declaration the plain
/// `__forward_` shim; these sites want the receiver-FIRST one
/// (`__fwdrecv_`, registered in `fnexpr_recv_fns`) so the real
/// receiver survives, and only this pass knows which they are.
pub(crate) fn recv_cb_claimed_sites(ast: &Ast) -> std::collections::HashSet<usize> {
    let fn_sigs = collect_promoted_fn_sigs(ast);
    if fn_sigs.is_empty() {
        return std::collections::HashSet::new();
    }
    let mut sites = collect_sites(ast, &fn_sigs);
    collect_objlit_field_sites(ast, &fn_sigs, &mut sites);
    collect_any_let_init_sites(ast, &fn_sigs, &mut sites);
    collect_props_store_sites(ast, &fn_sigs, &mut sites);
    collect_props_push_sites(ast, &fn_sigs, &mut sites);
    collect_json_cb_sites(ast, &fn_sigs, &mut sites);
    collect_as_any_sites(ast, &fn_sigs, &mut sites);
    super::namedfn_recv_cb_from::collect_array_from_sites(ast, &fn_sigs, &mut sites);
    sites.into_iter().map(|(e, _)| e.0 as usize).collect()
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

/// Mint `__fwdrecv_<target>(__env, __this, ...user) { return
/// target(__this, ...user) }` and append the decl — the receiver-first
/// face of the one forwarder synthesizer (`forwarder_synth`), so the
/// argv relay, the `__torajs_argv` peel and the gen-argv tail are the
/// plain shell's.
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
    let decl = super::forwarder_synth::synth_forwarder_decl(
        ast,
        &fwd,
        target,
        &params,
        return_type,
        target_span,
        super::forwarder_synth::ShellFace::ReceiverFirst,
    );
    ast.stmts.push(decl);
    fwd
}
