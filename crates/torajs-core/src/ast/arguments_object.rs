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

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch, stmt_uses_dynamic_arguments,
    synth_arguments_local,
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
    /// Leave the node alone so the checker rejects it loudly — a
    /// closure VALUE's real argc needs the ABI face (recorded);
    /// folding the declared arity would be silent-wrong.
    KeepLoud,
}

pub fn desugar_arguments_object(ast: &mut Ast) {
    // Snapshot per-fn user-param names, indexed by FnDecl name. The
    // walk below mutates expression nodes in place using these
    // snapshots.
    use std::collections::{HashMap, HashSet};
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    // T-31 — fns where `arguments.length` is referenced. Restricted to
    // free top-level FnDecls (user_start == 0; closures with `__env`
    // and class methods with `__this` keep the old declared-arity fold
    // to avoid disturbing their dispatch ABI). Each such fn gets a
    // synthetic first param `__torajs_real_argc: number`, and every
    // direct-Ident-callee Call to it gets `Number(args.len())`
    // prepended below.
    let mut uses_real_argc: HashSet<String> = HashSet::new();
    // Chunk 613 — lifted closures (hidden `__env` first param); their
    // `arguments.length` is only foldable through the IIFE static-argc
    // path, otherwise it stays loud (ArgcMode::KeepLoud).
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

    // Chunk 613 — IIFE closure form: `(function (a, b) { ...
    // arguments.length ... })(1)` lifts to a `__closure_N` FnDecl
    // (with the hidden `__env` param) whose ONLY call site is the
    // Call wrapping the Closure placeholder — so the real argc is
    // statically that call's arg count. Same T-31 shape: inject
    // `__torajs_real_argc: number` as the first USER param (after
    // `__env`) and prepend the static count at the call site.
    let mut iife_real_argc: HashSet<String> = HashSet::new();
    let mut iife_call_sites: Vec<usize> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, .. } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*callee) else {
            continue;
        };
        let fn_name = fn_name.clone();
        let has_len = ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name, body, .. }
                if *name == fn_name && body_has_arguments_length(ast, body))
        });
        if has_len {
            iife_real_argc.insert(fn_name);
            iife_call_sites.push(i);
        }
    }

    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form
    // seed + binding safety walk (see collect_value_argc).
    let (value_real_argc, argc_locals) = collect_value_argc(ast, &env_fns, &iife_real_argc);
    ast.closure_argc_locals = argc_locals;

    // T-31 — inject `__torajs_real_argc: number` at param[0] for each
    // uses_real_argc fn. Done before the body-rewrite walk so the
    // typechecker (which runs after desugar) sees the new signature
    // and so the recursive `arguments.length` rewrite below can resolve
    // `Ident("__torajs_real_argc")` cleanly.
    if !uses_real_argc.is_empty() || !iife_real_argc.is_empty() || !value_real_argc.is_empty() {
        for s in ast.stmts.iter_mut() {
            if let Stmt::FnDecl { name, params, .. } = s {
                // Chunk 613 — IIFE closures put the synthetic param
                // AFTER the hidden `__env` (first USER param slot);
                // RFC 20260708 value-form closures share that slot.
                let at = if iife_real_argc.contains(name) || value_real_argc.contains(name) {
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
            }
        }
    }

    let stmts_clone: Vec<Stmt> = ast.stmts.clone();
    for (idx, stmt) in stmts_clone.iter().enumerate() {
        if let Stmt::FnDecl { name, body, .. } = stmt {
            let Some(params) = fn_params.get(name) else {
                continue;
            };
            let params = params.clone();
            let argc_mode = if uses_real_argc.contains(name)
                || iife_real_argc.contains(name)
                || value_real_argc.contains(name)
            {
                ArgcMode::Real
            } else if env_fns.contains(name) && body_has_arguments_length(ast, body) {
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
            let mut needs_materialize = false;
            for s in body {
                if stmt_uses_dynamic_arguments(ast, s) {
                    needs_materialize = true;
                    break;
                }
            }
            let new_body: Vec<Stmt> = body
                .iter()
                .map(|s| {
                    crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt(
                        ast, s, &params, argc_mode,
                    )
                })
                .collect();
            // Synthesize the local OUTSIDE the &mut ast.stmts borrow
            // (synth_arguments_local also takes &mut ast for add_expr).
            let synth_opt = if needs_materialize {
                Some(synth_arguments_local(ast, &params))
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

/// RFC 20260708-closure-argc-abi chunk 1 — collect the closure
/// VALUE form seed: length-only lifted closures whose value is
/// consumed exclusively through a safe binding chain. Returns
/// `(value_real_argc fns, argc-local binding names)` — see the
/// gate rationale in the body comments.
fn collect_value_argc(
    ast: &Ast,
    env_fns: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use std::collections::{HashMap, HashSet};
    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form: a
    // lifted closure that reads `arguments.length` and is NOT an IIFE
    // has no static call site, so the real argc travels at runtime
    // (the ssa_lower closure-local call arm prepends
    // ConstI64(user-arg count) for bindings in `argc_locals`). Two
    // gates keep this zero-silent-wrong:
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
    //    loudly, today's behavior): a call-emit site that doesn't
    //    know about the argc slot would feed the callee garbage.
    //    The HOF/param-infection face is the RFC's chunk 2.
    let mut value_real_argc: HashSet<String> = HashSet::new();
    // binding name → source lifted-closure fn name (aliases share
    // the source fn).
    let mut argc_candidates: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = s {
            if env_fns.contains(name)
                && !iife_real_argc.contains(name)
                && body_has_arguments_length(ast, body)
                && !body_has_non_length_arguments_touch(ast, body)
            {
                value_real_argc.insert(name.clone());
            }
        }
    }
    // Direct bindings (`const f = function(){...}`), then alias
    // closure (`const g = f`) — top-level stmts only in chunk 1;
    // nested-scope aliases fail the safety walk below and kill the
    // chain (conservative, stays loud).
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && value_real_argc.contains(fn_name)
        {
            argc_candidates.insert(name.clone(), fn_name.clone());
        }
    }
    loop {
        let mut grew = false;
        for s in &ast.stmts {
            if let Stmt::LetDecl { name, init, .. } = s
                && let Expr::Ident(src) = ast.get_expr(*init)
                && argc_candidates.contains_key(src)
                && !argc_candidates.contains_key(name)
            {
                argc_candidates.insert(name.clone(), argc_candidates[src].clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    // Safety walk: for each candidate binding b, every arena
    // occurrence of `Ident(b)` must be a Call callee or a
    // still-candidate alias init. Kill rounds run to a fixpoint
    // because removing an alias invalidates its source's legality.
    loop {
        let mut killed: HashSet<String> = HashSet::new();
        for b in argc_candidates.keys() {
            let b_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
                .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == b))
                .collect();
            let mut legal: HashSet<u32> = HashSet::new();
            for e in &ast.exprs {
                if let Expr::Call { callee, .. } = e
                    && b_eids.contains(&callee.0)
                {
                    legal.insert(callee.0);
                }
            }
            for s in &ast.stmts {
                if let Stmt::LetDecl { name, init, .. } = s
                    && b_eids.contains(&init.0)
                    && argc_candidates.contains_key(name)
                {
                    legal.insert(init.0);
                }
            }
            if b_eids.difference(&legal).next().is_some() {
                killed.insert(b.clone());
            }
        }
        if killed.is_empty() {
            break;
        }
        let killed_fns: HashSet<String> = argc_candidates
            .iter()
            .filter(|(b, _)| killed.contains(*b))
            .map(|(_, f)| f.clone())
            .collect();
        argc_candidates.retain(|_, f| !killed_fns.contains(f));
    }
    // Only fns whose closure value is consumed EXCLUSIVELY through a
    // surviving binding chain get the argc injection — a closure
    // passed directly as an argument (HOF form, RFC chunk 2), stored
    // in a container, etc. must NOT be reshaped: its call sites don't
    // know about the argc slot and would feed the body garbage.
    value_real_argc = argc_candidates.values().cloned().collect();
    let argc_locals: HashSet<String> = argc_candidates.keys().cloned().collect();
    (value_real_argc, argc_locals)
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
