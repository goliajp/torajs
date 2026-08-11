//! RFC 20260708 closure argc/argv tier collectors — split out of
//! `arguments_object.rs` (file-size limit) when the argv-face tier
//! landed. Three units:
//!
//! - `collect_value_argc` — length-only real-argc closure VALUE
//!   seed + HOF mono-track admit (RFC 20260708-closure-argc-abi).
//! - `collect_value_argv` — full-arguments tier seed (RFC
//!   20260708-closure-argv-face).
//!
//! The shared `safe_binding_chain` safety walk both tiers gate on
//! lives in the `arguments_object_chain` sibling.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
    body_has_unsafe_return_arguments,
};
use super::{Ast, Expr, Stmt};

/// RFC 20260708-closure-argc-abi chunk 1 — collect the closure
/// VALUE form seed: length-only lifted closures whose value is
/// consumed exclusively through a safe binding chain. Returns
/// `(value_real_argc fns, argc-local binding names)` — see the
/// gate rationale in the body comments.
pub(super) fn collect_value_argc(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::HashSet;
    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form: a
    // lifted closure that reads `arguments.length` and is NOT an IIFE
    // has no static call site, so the real argc travels at runtime
    // (the S1 hidden-ABI `__torajs_argc` slot every env-first call
    // path feeds; bindings in `argc_locals` get the checker's
    // beyond-arity admit). Two gates keep this zero-silent-wrong:
    //
    // 1. Only the length-only tier qualifies: a body that also reads
    //    `arguments[dynamic]` / spreads `...arguments` needs the arg
    //    VALUES (the argv face, recorded follow-up) and stays
    //    KeepLoud — admitting a beyond-arity call whose values are
    //    unreachable would be silent-wrong.
    // 2. Every binding the closure value can reach must be
    //    direct-call-or-alias only (`f(...)` / `const g = f`). Any
    //    other use — passed as an argument, stored in a container,
    //    returned, reassigned — kills the whole chain back to the
    //    fn, which then stays KeepLoud (checker rejects `arguments`
    //    loudly, today's behavior): an unseen call site would refuse
    //    the beyond-arity shape the tier exists to serve. The
    //    HOF/param-infection face is the RFC's chunk 2.
    let mut value_real_argc: HashSet<String> = HashSet::new();
    // binding name → source lifted-closure fn name (aliases share
    // the source fn).
    let argc_candidates: Vec<(String, String)>;
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s {
            if env_fns.contains(name)
                && !iife_real_argc.contains(name)
                && !excluded.contains(name)
                && body_has_arguments_length(ast, body)
                && !body_has_non_length_arguments_touch(ast, body)
            {
                value_real_argc.insert(name.clone());
            }
        }
    }
    // RFC chunk 2 (mono track) — a length-only closure passed
    // DIRECTLY as an argument to a fn whose receiving param is
    // unannotated: that param goes through implicit generics (__T),
    // and the monomorphizer instantiates the slot as `__cls(`; the
    // body's length read rides the S1 hidden argc the param-call
    // arm feeds. Two gates: the receiving param must
    // be unannotated (mono track — a typed param never sees the
    // argc-shaped signature), and the receiving body must consume it
    // by DIRECT CALL only (any other use — alias, re-pass, store —
    // reaches sites that don't know the argc slot).
    let mut hof_arg_fns: HashSet<String> = HashSet::new();
    // The receiving-fn gate below resolves the callee name `g` with
    // a top-level FnDecl `.any()` — name-only. A call site whose `g`
    // is really a LOCAL binding (param / fn-body let) shadowing a
    // same-named top-level fn would pass the gate while the actual
    // receiver knows nothing about the argc slot → ABI mismatch
    // (probe: SIGBUS). Guard: a callee name bound in any fn-local
    // position anywhere is not provably the top-level fn — skip
    // (over-kill only loses the optimization, never wrong).
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                fn_local_names.insert(p.name.clone());
            }
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(g) = ast.get_expr(*callee) else {
            continue;
        };
        if fn_local_names.contains(g) {
            continue;
        }
        for (pi, a) in args.iter().enumerate() {
            let Expr::Closure { fn_name, .. } = ast.get_expr(*a) else {
                continue;
            };
            if !value_real_argc.contains(fn_name) {
                continue;
            }
            // RFC 20260708-variadic — a variadic-annotated param
            // (`(...args: E[]) => R`) also admits: its call lane is
            // the boxed dual entry whose adapter feeds the REAL
            // argc, and the checker's rest-tail assignability keeps
            // the value inside variadic-typed slots (a static
            // declared-pair site can never receive it), so no
            // body-use gate is needed for that track.
            let ok = ast.stmts.iter().any(|s| {
                matches!(s, Stmt::FnDecl { name, params, body, .. }
                    if name == g
                        && ((params.get(pi).is_some_and(|p| p.type_ann.is_none())
                            && param_uses_are_direct_calls(ast, body, &params[pi].name))
                            || params.get(pi).is_some_and(|p| p
                                .type_ann
                                .as_deref()
                                .is_some_and(|a| a.contains("__rest(")))))
            });
            if ok {
                hof_arg_fns.insert(fn_name.clone());
            }
        }
    }

    // Direct bindings + alias closure + kill fixpoint — the shared
    // safety walk (also used by the argv-face tier below).
    argc_candidates = super::arguments_object_chain::safe_binding_chain(ast, |fn_name| {
        value_real_argc.contains(fn_name)
    });
    // Only fns whose closure value is consumed EXCLUSIVELY through a
    // surviving binding chain get the argc injection — plus the
    // RFC-chunk-2 mono-track form below. A closure stored in a
    // container, returned, etc. must NOT be reshaped: its call
    // sites don't know about the argc slot and would feed the body
    // garbage.
    let mut injected: HashSet<String> = argc_candidates.iter().map(|(_, f)| f.clone()).collect();
    injected.extend(hof_arg_fns);
    let argc_locals: HashSet<String> = argc_candidates.iter().map(|(b, _)| b.clone()).collect();
    (injected, argc_locals)
}

/// RFC 20260708-closure-argv-face — collect the full-arguments tier:
/// lifted closures whose body touches `arguments` in any non-length
/// form. RFC 20260801 knife 2 admitted the bare-escape shapes
/// (return / alias / call arg / member) too: the rewriter swaps the
/// bare `arguments` to the materialized `__torajs_arguments` array,
/// which every consumer then treats as an ordinary `any[]` (a bare
/// root return takes the moved mark — see
/// `consume_all_idents_in_return`). Only the pass-through
/// `return arguments[i]` shape stays KeepLoud (the elem box would
/// leave borrowing the array's stake — UAF once the array
/// scope-drops). The value must survive the same
/// direct-call-or-alias binding walk as the argc tier: every call
/// then rides the boxed dual entry (the checker mints the type
/// rest-tail), whose adapter feeds real argc + argv. Returns
/// `(argv fns, binding names)`.
pub(super) fn collect_value_argv(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    excluded: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::HashSet;
    let mut full: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && env_fns.contains(name)
            && !iife_real_argc.contains(name)
            && !excluded.contains(name)
            && body_has_non_length_arguments_touch(ast, body)
            && !body_has_unsafe_return_arguments(ast, body)
        {
            full.insert(name.clone());
        }
    }
    if full.is_empty() {
        return (HashSet::new(), HashSet::new());
    }
    let candidates =
        super::arguments_object_chain::safe_binding_chain(ast, |fn_name| full.contains(fn_name));
    let mut injected: HashSet<String> = candidates.iter().map(|(_, f)| f.clone()).collect();
    injected.extend(collect_hof_anon_argv(ast, &full));
    let locals: HashSet<String> = candidates.iter().map(|(b, _)| b.clone()).collect();
    (injected, locals)
}

/// Rotation 363 — the HOF anonymous-callback arm of the argv face:
/// `[].map(function () { return arguments[0]; })`. The inline
/// fn-expr has no binding, so the safe-binding-chain walk (which
/// exists to prove every reachable call site rides the boxed dual
/// entry) has nothing to walk — but the shape needs no proof: the
/// closure's ONLY consumer is the one callback slot, and that
/// consumer is a lane that feeds real argc/argv (the array inline
/// loops route argv-face callees through the boxed variadic call;
/// every non-array receiver — any-lane, struct method, typed-array
/// kernel — packs args through a boxed protocol already).
///
/// Admission gates, all mechanical:
/// - the method name is in the lane whitelist below (exactly the
///   channels whose inline loop got the boxed-downgrade routing —
///   admitting a channel that still direct-calls would feed the
///   reshaped signature garbage, the r362 SIGSEGV lesson);
/// - the closure name's ONLY arena reference is that one callback
///   slot (an alias / second use reaches sites the lane can't
///   vouch for — those stay on the binding-chain track);
/// - the body doesn't ride the fnexpr-this promotion (a `__this`
///   head param and a `__torajs_argv` head would fight for the
///   adapter's slot mapping — recorded loud face).
fn collect_hof_anon_argv(
    ast: &Ast,
    full: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    const HOF_ARGV_METHODS: [&str; 5] = ["map", "filter", "forEach", "reduce", "reduceRight"];
    let mut ref_counts: HashMap<&str, usize> = HashMap::new();
    for e in &ast.exprs {
        match e {
            Expr::Closure { fn_name, .. } if full.contains(fn_name) => {
                *ref_counts.entry(fn_name).or_insert(0) += 1;
            }
            Expr::Ident(n) if full.contains(n) => {
                *ref_counts.entry(n).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    let mut admitted: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { name: m, .. } = ast.get_expr(*callee) else {
            continue;
        };
        if !HOF_ARGV_METHODS.contains(&m.as_str()) {
            continue;
        }
        let Some(&a0) = args.first() else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(a0) else {
            continue;
        };
        if full.contains(fn_name)
            && ref_counts.get(fn_name.as_str()) == Some(&1)
            && !ast.fnexpr_recv_fns.contains(fn_name)
        {
            admitted.insert(fn_name.clone());
        }
    }
    admitted
}

/// RFC 20260708-closure-argc-abi chunk 2 — every arena occurrence of
/// `Ident(pname)` must be a Call callee. Conservative: same-named
/// idents anywhere in the program count, so an unrelated binding
/// with the param's name fails the gate (stays loud, never silent).
fn param_uses_are_direct_calls(ast: &Ast, _body: &[Stmt], pname: &str) -> bool {
    use std::collections::HashSet;
    let p_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
        .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == pname))
        .collect();
    let mut legal: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e
            && p_eids.contains(&callee.0)
        {
            legal.insert(callee.0);
        }
    }
    p_eids.difference(&legal).next().is_none()
}
