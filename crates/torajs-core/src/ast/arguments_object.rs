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
use super::arguments_object_static_argv::{
    collect_iife_static_argv, collect_named_static_argv, inject_iife_static_params,
};
use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_arguments_length_write,
    body_has_non_length_arguments_touch, stmt_uses_dynamic_arguments, synth_arguments_local,
    synth_arguments_local_argv,
};
use super::{Ast, Expr, Param, Stmt};

/// How `arguments.length` rewrites inside a given fn body (chunk 613).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArgcMode {
    /// Fn has the synthetic `__torajs_real_argc` param — read it.
    Real,
    /// Fold to the declared arity (legacy fallback; still serves
    /// class methods, whose ABI is untouched — recorded face).
    FoldArity,
    /// RFC 20260801-arguments-escape-face — IIFE static-argv face:
    /// the single call site's arg count is statically known, so
    /// `arguments.length` folds to it directly. Distinct from
    /// FoldArity because params.len() is wrong on both sides: an
    /// under-filled site has fewer args than declared params, and
    /// the over-arity side carries injected `__torajs_iife_extra_*`
    /// params. Bare `arguments` escapes rewrite to the materialized
    /// `__torajs_arguments` local under this mode.
    FoldTo(usize),
    /// Length-write knife (rotation 270) — a static-argv-face body
    /// that WRITES `arguments.length`: the FoldTo fold would (a)
    /// mint a literal in the write target ("invalid assignment
    /// target") and (b) keep answering the stale constant on every
    /// later read. Length reads AND writes ride the materialized
    /// `__torajs_arguments.length` instead — the array resize is
    /// the §10.4.2.4 approximation on this face (expansion fills
    /// holes, truncation drops slots), which is what the
    /// arguments-iterator tests observe. The payload still carries
    /// the static argc for the materialize take-count, and
    /// `...arguments` spreads swap to the live array rather than
    /// inline-expanding a stale prefix.
    LiveLength(usize),
    /// Leave the node alone so the checker rejects it loudly — a
    /// closure VALUE's real argc needs the ABI face (recorded);
    /// folding the declared arity would be silent-wrong.
    KeepLoud,
}

pub fn desugar_arguments_object(ast: &mut Ast) {
    // Stage helpers extracted chunk 767 (the pass had drifted past
    // the 200-line fn limit as argc/argv tiers stacked up).
    let shadowed = collect_arguments_shadowed_fns(ast);
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
    // Shadowed fns (a binding named `arguments` in the body — see
    // collect_arguments_shadowed_fns) never join any face.
    static_argv.retain(|n, _| !shadowed.contains(n));
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
    // seed + binding safety walk (see collect_value_argc).
    let (value_real_argc, argc_locals) =
        collect_value_argc(ast, &env_fns, &iife_real_argc, &shadowed);
    ast.closure_argc_locals = argc_locals;

    // RFC 20260708-closure-argv-face — full-arguments tier seed +
    // the same binding safety walk (see collect_value_argv).
    let (value_argv_fns, argv_locals) =
        collect_value_argv(ast, &env_fns, &iife_real_argc, &shadowed);
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
        if let Stmt::FnDecl { name, body, .. } = stmt {
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
                // Length-write bodies leave the fold — reads and
                // writes both ride the materialized array's live
                // `.length` (see ArgcMode::LiveLength).
                if body_has_arguments_length_write(ast, body) {
                    ArgcMode::LiveLength(n)
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
            let mut needs_materialize = is_argv_fn || matches!(argc_mode, ArgcMode::LiveLength(_));
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
                // FoldTo — the materialized array is exactly argc
                // long (arguments.length semantics): over-arity is
                // covered by the injected extras already in `params`;
                // an under-filled site takes the leading argc params.
                let take = match argc_mode {
                    ArgcMode::FoldTo(n) | ArgcMode::LiveLength(n) => n.min(params.len()),
                    _ => params.len(),
                };
                Some(synth_arguments_local(ast, &params[..take]))
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

/// T-31 — inject `__torajs_real_argc: number` at param[0] for each
/// uses_real_argc fn. Done before the body-rewrite walk so the
/// typechecker (which runs after desugar) sees the new signature
/// and so the recursive `arguments.length` rewrite can resolve
/// `Ident("__torajs_real_argc")` cleanly.
fn inject_argc_params(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    value_real_argc: &std::collections::HashSet<String>,
    value_argv_fns: &std::collections::HashSet<String>,
) {
    if uses_real_argc.is_empty()
        && iife_real_argc.is_empty()
        && value_real_argc.is_empty()
        && value_argv_fns.is_empty()
    {
        return;
    }
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, params, .. } = s {
            // Chunk 613 — IIFE closures put the synthetic param
            // AFTER the hidden `__env` (first USER param slot);
            // RFC 20260708 value-form closures share that slot.
            let at = if iife_real_argc.contains(name)
                || value_real_argc.contains(name)
                || value_argv_fns.contains(name)
            {
                1
            } else if uses_real_argc.contains(name) {
                0
            } else {
                continue;
            };
            params.insert(
                at,
                Param {
                    name: "__torajs_real_argc".into(),
                    type_ann: Some("number".into()),
                    default: None,
                    is_rest: false,
                },
            );
            // argv-face bodies also take the adapter's raw argv
            // pointer right after the argc slot.
            if value_argv_fns.contains(name) {
                params.insert(
                    at + 1,
                    Param {
                        name: "__torajs_argv".into(),
                        type_ann: Some("__argvptr()".into()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
        }
    }
}

/// T-31 + chunk 613 — prepend the argc argument at static call
/// sites: every direct-Ident call to a `uses_real_argc` top-level
/// fn gets `Number(args.len())` as new arg[0], and each qualifying
/// IIFE call site gets its static count (the closure's `__env` is
/// not part of the Call args, so the count lands in the injected
/// first-USER-param slot).
fn prepend_static_argc(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
    iife_call_sites: Vec<usize>,
) {
    // T-31 — arena walk: every Call whose callee is a direct Ident to
    // a uses_real_argc fn gets `Number(args.len())` prepended as new
    // arg[0]. args.len() at this point is the user-passed count BEFORE
    // T-28's trailing-undef pad runs in check.rs / ssa_lower. The
    // checker sees the prepended arg, accepts the call (the remaining
    // user params are all Any and qualify for T-28 pad), and ssa_lower
    // lowers the prepended Number as ConstI64 matching the callee's
    // `: number` first param.
    if !uses_real_argc.is_empty() {
        let n = ast.exprs.len();
        for i in 0..n {
            let (callee, args_clone) = match &ast.exprs[i] {
                Expr::Call { callee, args } => (*callee, args.clone()),
                _ => continue,
            };
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            if !uses_real_argc.contains(&name) {
                continue;
            }
            let argc = args_clone.len();
            let argc_lit = ast.add_expr(Expr::Number(argc as f64));
            let mut new_args = Vec::with_capacity(argc + 1);
            new_args.push(argc_lit);
            new_args.extend(args_clone);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }

    // Chunk 613 — prepend the static argc at each qualifying IIFE
    // call site (the closure's `__env` is not part of the Call args,
    // so the count lands in the injected first-USER-param slot).
    for i in iife_call_sites {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let args_clone = args.clone();
        let argc_lit = ast.add_expr(Expr::Number(args_clone.len() as f64));
        let mut new_args = Vec::with_capacity(args_clone.len() + 1);
        new_args.push(argc_lit);
        new_args.extend(args_clone);
        ast.exprs[i] = Expr::Call {
            callee,
            args: new_args,
        };
    }
}
