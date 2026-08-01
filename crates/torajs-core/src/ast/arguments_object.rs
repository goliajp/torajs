//! T-11 / T-31 — `arguments` object desugar pass + its 5 yield-free
//! walkers + synthesized-local builder.
//!
//! Chunk 345 — paired with chunk 344's `arguments_object_rewrite`
//! sibling. The main pass (`desugar_arguments_object`) keeps the
//! `pub` symbol (re-exported by ast.rs for the three external
//! callers in `crates/torajs-cli/src/{cmd_build,main,lsp}.rs`), and
//! the helpers (`stmt_uses_dynamic_arguments` / `expr_uses_dynamic_arguments`
//! / `body_has_arguments_length` / `stmt_has_arguments_length` /
//! `expr_has_arguments_length` / `synth_arguments_local`) stay
//! sibling-private here.
//!
//! The cross-sibling call to the actual rewriters lives at
//! `crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt`.

use super::arguments_object_collect::{collect_value_argc, collect_value_argv};
use super::arguments_object_inject::{inject_argc_params, prepend_static_argc};
use super::arguments_object_static_argv::{
    collect_iife_static_argv, collect_method_static_argv, collect_named_static_argv,
    collect_objlit_method_static_argv, inject_iife_static_params,
};
use super::arguments_object_synth::{
    synth_arguments_local, synth_arguments_local_argv, synth_arguments_local_rest,
};
use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_arguments_length_write, body_has_bare_arguments_assign,
    body_has_non_length_arguments_touch, stmt_uses_dynamic_arguments,
};
use super::{Ast, Expr, Stmt};

/// How `arguments.length` rewrites inside a given fn body (chunk 613).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArgcMode {
    /// Fn has the synthetic `__torajs_real_argc` param — read it.
    Real,
    /// Fold to the declared arity (legacy fallback; still serves
    /// class methods, whose ABI is untouched — recorded face).
    FoldArity,
    /// Mutation-free FoldTo (rotation 272 re-admission) — a
    /// static-argv-face body with SIMPLE params that never writes an
    /// arguments element, never writes a param, and never lets the
    /// arguments object escape (see arguments_object_mutation): the
    /// zero-cost literal-index param substitution and inline spread
    /// expansion are then observationally equivalent to the real
    /// unmapped arguments object — nothing can tell the snapshot
    /// from the live view — AND they preserve static element types
    /// (an `...arguments` inline expansion stays `number`-typed
    /// where the materialized `any[]` ride would smuggle boxed
    /// slots into a typed literal). Any mutation / escape / non-
    /// simple param routes to Unmapped instead.
    FoldTo(usize),
    /// Length-write knife (rotation 270) — a static-argv-face body
    /// that WRITES `arguments.length`: a static fold would (a)
    /// mint a literal in the write target ("invalid assignment
    /// target") and (b) keep answering the stale constant on every
    /// later read. Length reads AND writes ride the materialized
    /// `__torajs_arguments.length` instead — the array resize is
    /// the §10.4.2.4 approximation on this face (expansion fills
    /// holes, truncation drops slots), which is what the
    /// arguments-iterator tests observe. The payload still carries
    /// the static argc for the materialize take-count, and
    /// `...arguments` spreads swap to the live array rather than
    /// inline-expanding a stale prefix. Element indices ride the
    /// array too — see Unmapped below for why no mode may alias.
    LiveLength(usize),
    /// The static-argv face's standard mode (rotation 271 introduced
    /// it for default / rest / destructured params per ES
    /// §10.4.4.6/7; rotation 272 made it the whole face's mode):
    /// tr's semantic baseline is bun running TS, and a TS file
    /// executes as an ES module — always strict code — where the
    /// arguments object is UNMAPPED for every fn, simple params
    /// included. The old mapped literal-index param substitution
    /// diverged both ways (`arguments[0] = 2` wrote through to `a`;
    /// `a = 99` showed in a later `arguments[0]` read). Reads and
    /// writes both ride the materialized `__torajs_arguments` array
    /// instead; `arguments.length` still folds to the static argc.
    Unmapped(usize),
    /// Leave the node alone so the checker rejects it loudly — a
    /// closure VALUE's real argc needs the ABI face (recorded);
    /// folding the declared arity would be silent-wrong.
    KeepLoud,
}

pub fn desugar_arguments_object(ast: &mut Ast) {
    // Pre-pass — rewrite exclusively-called fn-value aliases into
    // direct calls so the face analyses below see their sites (and
    // the forwarder relay's arg drop is bypassed). See the module
    // doc in arguments_object_devirt.
    super::arguments_object_devirt::devirtualize_fn_value_aliases(ast);
    // Stage helpers extracted chunk 767 (the pass had drifted past
    // the 200-line fn limit as argc/argv tiers stacked up).
    let shadowed = collect_arguments_shadowed_fns(ast);
    // Bare-assign bodies (`arguments = v`, for-await dstr defaults)
    // also leave every swapping face — the materialized local is
    // const, and the pre-face undeclared-ident lane (sloppy
    // auto-global) is the behavior those tests observe. They still
    // take the default FoldArity rewrite (literal-index param
    // substitution, pre-face parity) — only the face admissions are
    // gated, so `excluded` feeds the collectors while `shadowed`
    // alone skips the rewrite loop.
    let mut excluded = shadowed.clone();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s
            && !excluded.contains(name)
            && body_has_bare_arguments_assign(ast, body)
        {
            excluded.insert(name.clone());
        }
    }
    let (mut fn_params, uses_real_argc, env_fns) = snapshot_fn_params(ast);
    let (iife_real_argc, iife_call_sites) = collect_iife_real_argc(ast, &shadowed);

    // RFC 20260801-arguments-escape-face knives 1+3a — static-argv
    // face: any non-length arguments touch (index / spread / bare
    // escape) resolves fully statically when every call site is
    // known and passes the same arg count: an IIFE's single site
    // (knife 1), or a top-level named fn whose every reference is a
    // direct call with one uniform argc (knife 3a). Injects trailing
    // `__torajs_static_extra_*: any` params so over-arity args reach
    // the body, and extends the rewrite param table to match.
    let mut static_argv = collect_iife_static_argv(ast, &iife_real_argc);
    static_argv.extend(collect_named_static_argv(ast, &env_fns));
    // RFC 20260801-arguments-method-face knife 1 — single-owner
    // class methods ride the same face (receiver slot excluded from
    // the argc count).
    static_argv.extend(collect_method_static_argv(ast));
    // RFC 20260801 objlit branch — object-literal methods whose
    // member call sites are all visible and uniform.
    static_argv.extend(collect_objlit_method_static_argv(ast, &env_fns));
    // Shadowed fns (a binding named `arguments` in the body — see
    // collect_arguments_shadowed_fns) and bare-assign fns never
    // join any face.
    static_argv.retain(|n, _| !excluded.contains(n));
    let iife_static_argv = static_argv;
    inject_iife_static_params(ast, &iife_static_argv, &mut fn_params);
    // A named fn admitted to the static face must leave the T-31
    // real-argc tier (param injection + call-site argc prepend would
    // double-reshape its signature); a shadowed fn leaves every tier.
    let uses_real_argc: std::collections::HashSet<String> = uses_real_argc
        .into_iter()
        .filter(|n| !iife_static_argv.contains_key(n) && !shadowed.contains(n))
        .collect();

    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form
    // seed + binding safety walk (see collect_value_argc). A closure
    // the static face admitted (objlit branch) must leave the value
    // tiers — the face already reshaped its signature, and the value
    // adapters would double-reshape it.
    let (mut value_real_argc, argc_locals) =
        collect_value_argc(ast, &env_fns, &iife_real_argc, &excluded);
    value_real_argc.retain(|n| !iife_static_argv.contains_key(n));
    ast.closure_argc_locals = argc_locals;

    // RFC 20260708-closure-argv-face — full-arguments tier seed +
    // the same binding safety walk (see collect_value_argv).
    let (mut value_argv_fns, argv_locals) =
        collect_value_argv(ast, &env_fns, &iife_real_argc, &excluded);
    value_argv_fns.retain(|n| !iife_static_argv.contains_key(n));
    ast.closure_argv_fns = value_argv_fns.clone();
    ast.closure_argv_locals = argv_locals;

    inject_argc_params(
        ast,
        &uses_real_argc,
        &iife_real_argc,
        &value_real_argc,
        &value_argv_fns,
    );

    let stmts_clone: Vec<Stmt> = ast.stmts.clone();
    for (idx, stmt) in stmts_clone.iter().enumerate() {
        if let Stmt::FnDecl {
            name,
            params: decl_params,
            body,
            ..
        } = stmt
        {
            // A shadowed fn's `arguments` idents refer to its OWN
            // `var arguments` local — leave the body untouched (the
            // hoisted decl resolves them as an ordinary binding).
            if shadowed.contains(name) {
                continue;
            }
            let Some(params) = fn_params.get(name) else {
                continue;
            };
            let params = params.clone();
            let is_argv_fn = value_argv_fns.contains(name);
            // RFC 20260708-closure-argv-face chunk 2 — an env
            // closure that did NOT qualify for the argv face (killed
            // binding chain / escaping touch / unsafe return) stays
            // loud on EVERY arguments touch, index reads included:
            // the historical FoldArity path rewrote `arguments[i]`
            // into a declared-params-only array, silently answering
            // undefined for beyond-declared values reached through
            // any-call / container dispatch (probe ac6/ac7).
            let argc_mode = if let Some(&n) = iife_static_argv.get(name) {
                // Length-write bodies additionally ride the array's
                // live `.length` (see ArgcMode::LiveLength);
                // everything else on the face is Unmapped — strict
                // (module) code never maps arguments to params.
                if body_has_arguments_length_write(ast, body) {
                    ArgcMode::LiveLength(n)
                } else if decl_params.iter().any(|p| {
                    p.default.is_some() || p.is_rest || p.name.starts_with("__param_destr_")
                }) || super::arguments_object_mutation::body_mutates_args_view(
                    ast, body, &params,
                ) {
                    // Non-simple params misalign position ↔ param;
                    // a mutation / escape makes the snapshot
                    // distinguishable (see ArgcMode::FoldTo doc).
                    ArgcMode::Unmapped(n)
                } else {
                    ArgcMode::FoldTo(n)
                }
            } else if uses_real_argc.contains(name)
                || iife_real_argc.contains(name)
                || value_real_argc.contains(name)
                || is_argv_fn
            {
                ArgcMode::Real
            } else if env_fns.contains(name)
                && (body_has_arguments_length(ast, body)
                    || body_has_non_length_arguments_touch(ast, body))
            {
                ArgcMode::KeepLoud
            } else if super::arguments_object_mutation::body_mutates_args_view(ast, body, &params) {
                // FoldArity tier (no static argc), mutating body —
                // the literal-index substitution wrote through
                // (`arguments[0] = 5` mutated `a`; module code is
                // strict, so nothing ever maps). Ride the
                // materialized array under the tier's existing
                // declared-==-actual assumption: length folds and
                // beyond-declared reads stay wrong-at-parity with
                // the FoldArity baseline, but writes are isolated.
                ArgcMode::Unmapped(params.len())
            } else {
                ArgcMode::FoldArity
            };
            // T-11 — pre-pass: detect any dynamic `arguments[<non-
            // literal>]` use. If found, prepend a synthesized
            // `let __torajs_arguments: any[] = [p0, p1, ...]` before
            // the body and rewrite the dynamic indices to read from it.
            // Literal-index rewrites (the existing path) take priority
            // and don't materialize the array — they stay zero-cost.
            // argv-face bodies always materialize (from the
            // adapter's argv — beyond-declared values included);
            // named-fn bodies keep the declared-params builder,
            // gated on a dynamic use.
            let mut needs_materialize =
                is_argv_fn || matches!(argc_mode, ArgcMode::LiveLength(_) | ArgcMode::Unmapped(_));
            if !needs_materialize {
                for s in body {
                    if stmt_uses_dynamic_arguments(ast, s) {
                        needs_materialize = true;
                        break;
                    }
                }
            }
            let new_body: Vec<Stmt> = body
                .iter()
                .map(|s| {
                    crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt(
                        ast, s, &params, argc_mode, is_argv_fn,
                    )
                })
                .collect();
            // Synthesize the local OUTSIDE the &mut ast.stmts borrow
            // (synth_arguments_local also takes &mut ast for add_expr).
            let synth_opt = if is_argv_fn {
                Some(synth_arguments_local_argv(ast))
            } else if needs_materialize && argc_mode != ArgcMode::KeepLoud {
                Some(synth_materialized_arguments(
                    ast,
                    decl_params,
                    &params,
                    argc_mode,
                ))
            } else {
                None
            };
            if let Stmt::FnDecl { body: b, .. } = &mut ast.stmts[idx] {
                if let Some(synth) = synth_opt {
                    let mut full = Vec::with_capacity(new_body.len() + 1);
                    full.push(synth);
                    full.extend(new_body);
                    *b = full;
                } else {
                    *b = new_body;
                }
            }
        }
    }

    prepend_static_argc(ast, &uses_real_argc, iife_call_sites);
}

/// Build the materialized `__torajs_arguments` local for a fn under
/// a materializing mode. A rest-tailed fn spreads the rest array
/// (over-arity values live there — no extras were injected, see
/// inject_iife_static_params); everyone else takes the positional
/// builder over the leading `argc` params: over-arity is covered by
/// the injected extras already in `params`, an under-filled site
/// contributes only the args it actually passed.
fn synth_materialized_arguments(
    ast: &mut Ast,
    decl_params: &[super::Param],
    params: &[String],
    argc_mode: ArgcMode,
) -> Stmt {
    if decl_params.last().is_some_and(|p| p.is_rest)
        && let Some((rest_name, fixed)) = params.split_last()
    {
        let take = match argc_mode {
            ArgcMode::FoldTo(n) | ArgcMode::LiveLength(n) | ArgcMode::Unmapped(n) => {
                n.min(fixed.len())
            }
            _ => fixed.len(),
        };
        synth_arguments_local_rest(ast, &fixed[..take], rest_name)
    } else {
        let take = match argc_mode {
            ArgcMode::FoldTo(n) | ArgcMode::LiveLength(n) | ArgcMode::Unmapped(n) => {
                n.min(params.len())
            }
            _ => params.len(),
        };
        synth_arguments_local(ast, &params[..take])
    }
}

/// Sloppy-mode shadow guard (rotation 270, test262 10.6-6-3/4) — a
/// fn whose param list or body declares a binding named `arguments`
/// (`var arguments = ...`): every `arguments` ident inside refers to
/// that local, so the fn must leave EVERY argc/argv tier and stay
/// unrewritten (the pre-face behavior). Without this, the named
/// static-argv face admitted such fns and the bare-escape swap
/// turned `arguments = undefined` into an assignment to the const
/// synth local. `collect_local_binding_names` recurses into nested
/// FnDecls, so a nested declaration over-excludes the outer fn —
/// conservative (loses an admit, never wrong).
fn collect_arguments_shadowed_fns(ast: &Ast) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            let mut names = std::collections::HashSet::new();
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut names);
            if params.iter().any(|p| p.name == "arguments") || names.contains("arguments") {
                out.insert(name.clone());
            }
        }
    }
    out
}

/// Snapshot per-fn user-param names, indexed by FnDecl name (the
/// rewrite walk mutates expression nodes in place using these).
/// Also answers:
///
/// - T-31 `uses_real_argc` — fns where `arguments.length` is
///   referenced, restricted to free top-level FnDecls (user_start
///   == 0; closures with `__env` and class methods with `__this`
///   keep the old declared-arity fold to avoid disturbing their
///   dispatch ABI). Each such fn gets a synthetic first param
///   `__torajs_real_argc: number`, and every direct-Ident-callee
///   Call to it gets `Number(args.len())` prepended.
/// - Chunk 613 `env_fns` — lifted closures (hidden `__env` first
///   param); their `arguments.length` is only foldable through the
///   IIFE static-argc path, otherwise ArgcMode::KeepLoud.
#[allow(clippy::type_complexity)]
fn snapshot_fn_params(
    ast: &Ast,
) -> (
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::{HashMap, HashSet};
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut uses_real_argc: HashSet<String> = HashSet::new();
    let mut env_fns: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            // Skip the synthetic `__env` (closure capture vector) and
            // `__this` (class instance) prefix params — they're not
            // user-visible "arguments". Everything after is the
            // user's declared param list.
            let user_start = params
                .first()
                .filter(|p| p.name == "__env" || p.name == "__this")
                .map(|_| 1)
                .unwrap_or(0);
            let names: Vec<String> = params[user_start..]
                .iter()
                .map(|p| p.name.clone())
                .collect();
            fn_params.insert(name.clone(), names);
            if params.first().is_some_and(|p| p.name == "__env") {
                env_fns.insert(name.clone());
            }
            if user_start == 0 && body_has_arguments_length(ast, body) {
                uses_real_argc.insert(name.clone());
            }
        }
    }
    (fn_params, uses_real_argc, env_fns)
}

/// Chunk 613 — IIFE closure form: `(function (a, b) { ...
/// arguments.length ... })(1)` lifts to a `__closure_N` FnDecl
/// (with the hidden `__env` param) whose ONLY call site is the
/// Call wrapping the Closure placeholder — so the real argc is
/// statically that call's arg count. Same T-31 shape: inject
/// `__torajs_real_argc: number` as the first USER param (after
/// `__env`) and prepend the static count at the call site.
fn collect_iife_real_argc(
    ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
) -> (std::collections::HashSet<String>, Vec<usize>) {
    let mut iife_real_argc = std::collections::HashSet::new();
    let mut iife_call_sites: Vec<usize> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, .. } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        if shadowed.contains(fn_name) {
            continue;
        }
        let fn_name = fn_name.clone();
        // RFC 20260801 knife 1 narrowed this tier to length-ONLY
        // bodies: any non-length touch (index / spread / escape)
        // routes to the static-argv face instead, which serves
        // length AND values fully statically.
        let has_len = ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name, body, .. }
                if *name == fn_name && body_has_arguments_length(ast, body)
                    && !body_has_non_length_arguments_touch(ast, body))
        });
        if has_len {
            iife_real_argc.insert(fn_name);
            iife_call_sites.push(i);
        }
    }
    (iife_real_argc, iife_call_sites)
}
