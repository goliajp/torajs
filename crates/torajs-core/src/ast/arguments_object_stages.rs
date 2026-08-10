//! The stage collectors of
//! [`super::arguments_object::desugar_arguments_object`] — the
//! shadow guard, the per-fn param snapshot, and the IIFE real-argc
//! seed. Split to a sibling when the S2 sloppy-callee knives (RFC
//! 20260810-sloppy-goal-arguments) pushed the main pass past the
//! 500-line file limit (verbatim move).

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};

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
pub(super) fn collect_arguments_shadowed_fns(ast: &Ast) -> std::collections::HashSet<String> {
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
pub(super) fn snapshot_fn_params(
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
            // `__this` (receiver) prefix params — they're not
            // user-visible "arguments". A lifted ctor fn-expr carries
            // BOTH (`__closure_N(__env, __this, …user)` — the ctor-this
            // promotion runs before the lift), so the skip is a chain,
            // not an either/or. Everything after is the user's
            // declared param list.
            let mut user_start = usize::from(params.first().is_some_and(|p| p.name == "__env"));
            if params.get(user_start).is_some_and(|p| p.name == "__this") {
                user_start += 1;
            }
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
pub(super) fn collect_iife_real_argc(
    ast: &Ast,
    shadowed: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut iife_real_argc = std::collections::HashSet::new();
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
        }
    }
    iife_real_argc
}
