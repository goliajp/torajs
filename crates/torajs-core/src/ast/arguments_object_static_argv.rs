//! RFC 20260801-arguments-escape-face — the static-argv face
//! collectors + param injector for
//! [`super::arguments_object::desugar_arguments_object`], split to a
//! sibling when knife 3a pushed the main pass file past the 500-line
//! limit. Knife 1: an IIFE's single call site makes the whole argv
//! static. Knife 3a: a top-level named fn whose every reference is a
//! direct call with one uniform argc joins the same face.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};

/// RFC 20260801-arguments-escape-face knife 1 — collect IIFEs whose
/// body touches `arguments` in any non-length form. The single call
/// site makes the whole argv static: `fn name → call-site arg count`.
/// A fn spotted at two call sites is skipped defensively (lifted
/// IIFE closures have exactly one; two means this isn't the shape we
/// think it is — over-kill only loses the admit, never wrong).
///
/// The inline `new (function () { …arguments[i]… })(…)` twin joins
/// the same face: a NewDynamic callee's closure value exists only at
/// this construct site, and the construct channel's boxed dual entry
/// feeds every declared param (injected extras included) from the
/// site's argument list, so the static fold answers the same truth a
/// direct call would.
///
/// A site carrying a dynamic `...spread` poisons the name: args.len()
/// counts the spread as ONE argument, so the injected extras would
/// mis-slot every value after it (silent) — the fn stays off the
/// face (the named-fn collector's own spread rule).
pub(super) fn collect_iife_static_argv(
    ast: &Ast,
    iife_real_argc: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};
    let mut argc: HashMap<String, usize> = HashMap::new();
    let mut dup: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let (callee, args) = match e {
            Expr::Call { callee, args } => (callee, args),
            Expr::NewDynamic { callee, args } => (callee, args),
            _ => continue,
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        if args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            dup.insert(fn_name.clone());
            continue;
        }
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
///
/// A `__this`-first fn (RFC 20260726 blade 1 gave constructible
/// plain functions the receiver param; bind_this_param prepends the
/// receiver at EVERY direct call site, so the slot is uniformly
/// occupied) joins with the receiver excluded from the argc count —
/// the `__cm_` method precedent. Length-only bodies qualify on this
/// shape too: the T-31 real-argc tier only takes user_start == 0
/// fns, so without the face a constructed fn's `arguments.length`
/// FoldArity-folds to the declared arity — 0 for the classic
/// `function H() { this.n = arguments.length }` — while the factory
/// (fn_constructor_argc) now forwards the construct site's full
/// argument list.
pub(super) fn collect_named_static_argv(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};
    let mut cand: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        // A class constructor's only call by name is its factory's,
        // which passes the declared parameters rather than the
        // program's arguments; the method face votes at the `new` and
        // `super(…)` sites instead (`ctor_uniform_argc`). Voting here
        // also counted the new-target slot as an argument.
        if env_fns.contains(name) || (name.starts_with("__cm_") && name.ends_with("__ctor")) {
            continue;
        }
        // the head is `__this`, plus `__new_target` on a declaration
        // that reads `new.target` (`new_target_param`)
        let this_slots = match params.first() {
            Some(p) if p.name == "__this" => {
                1 + usize::from(params.get(1).is_some_and(|p| p.name == "__new_target"))
            }
            _ => 0,
        };
        let touches = body_has_non_length_arguments_touch(ast, body)
            || (this_slots > 0 && body_has_arguments_length(ast, body));
        if touches {
            cand.insert(name.clone(), this_slots);
        }
    }
    if cand.is_empty() {
        return HashMap::new();
    }
    // A same-named PARAM of another fn is resolved per body by the
    // reference walk; any other fn-local spelling still refuses.
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { body, .. } = s {
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    let fwd_sites = forwarder_call_callee_eids(ast);
    let meta_reads = super::arguments_object_walkers::fn_meta_read_receivers(ast);
    let mut out: HashMap<String, usize> = HashMap::new();
    for (name, this_slots) in cand {
        if fn_local_names.contains(&name) {
            continue;
        }
        if let Some(n) = uniform_direct_call_argc(ast, &name, this_slots, &fwd_sites, &meta_reads) {
            out.insert(name, n);
        }
    }
    out
}

/// Shared reference analysis (knife 3a + method face): every arena
/// `Ident(name)` resolving to the fn (a same-named param's uses do
/// not) must be a direct Call callee (or the receiver of a
/// `.length` / `.name` metadata read), and all sites must
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
    meta_reads: &std::collections::HashSet<u32>,
) -> Option<usize> {
    use std::collections::HashSet;
    let shadowed = super::arguments_object_walkers::param_shadowed_eids(ast, name);
    let eids: HashSet<u32> = (0..ast.exprs.len() as u32)
        .filter(|&i| {
            matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == name) && !shadowed[i as usize]
        })
        .collect();
    if eids.is_empty() {
        return None;
    }
    let mut argcs: HashSet<usize> = HashSet::new();
    let mut legal: HashSet<u32> = meta_reads.intersection(&eids).copied().collect();
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
pub(super) fn forwarder_call_callee_eids(ast: &Ast) -> std::collections::HashSet<u32> {
    let mut out = std::collections::HashSet::new();
    for s in &ast.stmts {
        // A receiver-first relay (`__fwdrecv_`, rotation 649) is the
        // same single forwarding call and just as much not a USER
        // site; counted, it folded a `this`-reading declaration used
        // only at thisArg callback positions to its declared arity.
        if let Stmt::FnDecl { name, body, .. } = s
            && (name.starts_with("__forward_") || name.starts_with("__fwdrecv_"))
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
    let meta_reads = super::arguments_object_walkers::fn_meta_read_receivers(ast);
    // a class the construct channel reaches is built from arguments
    // no `new` site spells (`new (C as any)(…)`, `Reflect.construct`)
    let from_value = crate::ssa_lower_boxed_entry::program_constructs_from_value(ast);
    let heirs = crate::ssa_lower_boxed_entry::promise_heir_classes(ast);
    let mut out: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        // Knife 5 (rotation 273) — static-method forwarders
        // (`__sm_<C>__<m>`, `__this: any` first, receiver is the
        // class object / undefined) join too: a static generator
        // method's forwarder carries the knife-2b `[...arguments]`
        // tail, and without a face it folded to a statically empty
        // argv (probe: `C.gm(42,'TC39')` answered length 0).
        if !name.starts_with("__cm_") && !name.starts_with("__sm_") {
            continue;
        }
        if params.first().is_none_or(|p| p.name != "__this") {
            continue;
        }
        if params.iter().any(|p| p.default.is_some() || p.is_rest) {
            continue;
        }
        if ast.method_owners.iter().any(|(m, owners)| {
            owners
                .iter()
                .any(|o| name == &format!("__cm_{o}__{m}") || name == &format!("__sm_{o}__{m}"))
        }) {
            continue;
        }
        if !body_has_non_length_arguments_touch(ast, body)
            && !super::arguments_object_walkers::body_has_arguments_length(ast, body)
        {
            continue;
        }
        // A constructor's direct call carries TWO leading slots, not
        // one: the factory writes `__cm_C__ctor(__this,
        // __torajs_my_class_ref("C"), …)`. Counting the new-target
        // slot as a user argument put one phantom entry in every
        // ctor's arguments object (`new P(1)` answered
        // `arguments.length === 2`).
        let recv_slots = 1 + usize::from(params.get(1).is_some_and(|p| p.name == "__new_target"));
        // A constructor's own direct call lives inside its factory
        // and carries only what the FACTORY declares, so voting there
        // answers the ctor's declared arity and never the program's:
        // `new P(1, 2, 3)` on `constructor(a)` voted 1. The arguments
        // the program wrote are at the `new` site, which desugars to
        // `__new_<C>(…)` — that is where the count lives, with no
        // receiver slots in front of it.
        let argc = match name
            .strip_prefix("__cm_")
            .and_then(|r| r.strip_suffix("__ctor"))
            .filter(|_| recv_slots == 2)
        {
            Some(cname)
                if from_value
                    || crate::ssa_lower_boxed_entry::factory_always_admitted(
                        ast, &heirs, cname,
                    ) =>
            {
                None
            }
            Some(cname) => ctor_uniform_argc(ast, name, cname, &fwd_sites, &meta_reads),
            None => uniform_direct_call_argc(ast, name, recv_slots, &fwd_sites, &meta_reads),
        };
        if let Some(n) = argc {
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
/// The constructor face's argc vote. Two kinds of site write a
/// ctor's arguments and neither is the ctor's own name:
///
/// - `new C(…)`, which desugars to `__new_C(…)` — no receiver slots;
/// - `super(…)` in a subclass ctor, which desugars to a direct
///   `__cm_C__ctor(__this, <new-target>, …)` — two.
///
/// The one call that must NOT vote is the factory's own, since it
/// carries the FACTORY's declared params rather than the program's
/// arguments; voting there answered the declared arity for every
/// ctor (`new P(1, 2, 3)` on `constructor(a)` voted 1). It is
/// identified by ownership: the expressions the `__new_C` body owns.
fn ctor_uniform_argc(
    ast: &Ast,
    ctor: &str,
    cname: &str,
    fwd_sites: &std::collections::HashSet<u32>,
    meta_reads: &std::collections::HashSet<u32>,
) -> Option<usize> {
    let factory = format!("__new_{cname}");
    let mut inner = vec![false; ast.exprs.len()];
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && *name == factory
        {
            super::desugar_eval::body_owned_exprs(ast, body, &mut inner);
        }
    }
    let mut argcs: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let factory_used = ast
        .exprs
        .iter()
        .any(|e| matches!(e, Expr::Ident(n) if *n == factory));
    if factory_used {
        argcs.insert(uniform_direct_call_argc(
            ast, &factory, 0, fwd_sites, meta_reads,
        )?);
    }
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        if inner[i] {
            continue;
        }
        if !matches!(ast.get_expr(*callee), Expr::Ident(n) if n == ctor) {
            continue;
        }
        if args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            return None;
        }
        argcs.insert(args.len().saturating_sub(2));
    }
    if argcs.len() == 1 {
        argcs.into_iter().next()
    } else {
        None
    }
}
