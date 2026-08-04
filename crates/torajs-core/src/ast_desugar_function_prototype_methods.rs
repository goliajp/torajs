//! `desugar_function_prototype_methods` extracted from
//! [`crate::ast`] (chunk 138).
//!
//! Pre-extract this was a 221 LOC `pub fn` inline in ast.rs (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here; the ast.rs side keeps a 1-line
//! wrapper `pub fn desugar_function_prototype_methods` that
//! delegates so existing `ast::desugar_function_prototype_methods`
//! callsites (torajs-cli main / lsp / repl / cmd_build) stay
//! source-compatible.
//!
//! Rewrites three Function.prototype helpers:
//!
//! - **`.call(thisArg, a1, a2)`** → `f(a1, a2)` (thisArg dropped per
//!   our subset; the bare `f.call()` form is `f()`).
//! - **`.apply(thisArg, [a, b])`** → `f(a, b)` (array-literal 2nd
//!   arg unpacks; absent / `undefined` / `null` argArray is `f()`
//!   per §22.2.3.1; dynamic arrays stay unsupported here).
//! - **`.bind(thisArg, p0, p1, ...)`** →
//!   `__bind_create_<id>(p0, p1, ...)` — leading with the thisArg
//!   when the target is this-using, since its receiver is param 0
//!   and binding it is just the first partial. Synthesizes a wrapper
//!   `__bound_<id>(__env(captures), rem_params...) -> R` plus a
//!   factory `__bind_create_<id>(captures...) -> __cls(rem_tys)->R`
//!   that returns a Closure capturing the partial args. The wrapper
//!   forwards its `__env` captures + new args to the original fn.
//!   Factory FnDecl pushed BEFORE the bound FnDecl so ssa_lower
//!   sees the Closure construction site (which carries the capture
//!   types) before lowering the bound body that reads them.
//!
//! Skips fn sources whose first param is `__env` (closure-shaped
//! sources need env-forwarding through the factory — post-MVP).
//! Skips already-synthesized helpers (`__bound_` / `__bind_create_`
//! prefixes) so repeat invocations don't double-rewrite.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Whether this pass will rewrite `f.<m_name>(args)` for a plain
/// top-FnDecl `f` declaring `fn_params`. The single source of
/// truth the collector's receiver-wrap axis consults: a form this
/// pass swallows becomes a direct call (receiver must NOT wrap, or
/// the Ident match below would stop seeing it); a form it leaves —
/// dynamic argArray, surplus bind partials — keeps the member call,
/// whose fn-name receiver then rides the wrapped closure lane.
/// The `__smany_` twin that should receive `f.<call|apply|bind>(…)`
/// in place of a this-using static's mono, or `None` when the mono
/// stands. `twin_exists` probes the caller's own fn-signature map.
///
/// A static's mono has no receiver channel at all (knife 2 binds its
/// `this` to the class name), so a rebind can only be served by the
/// twin — and BOTH the rewrite and the collector's receiver-wrap axis
/// have to agree on which signature is in play, or they disagree
/// about whether the form is swallowed. Hence one shared resolver
/// rather than the test living at either call site.
pub(crate) fn retarget_static_twin(
    fn_name: &str,
    fn_params: &[Param],
    twin_exists: impl FnOnce(&str) -> bool,
) -> Option<String> {
    if fn_params.first().is_some_and(|p| p.name == "__this") {
        return None;
    }
    let rest = fn_name.strip_prefix("__sm_")?;
    let twin = format!("__smany_{rest}");
    twin_exists(&twin).then_some(twin)
}

pub(crate) fn swallows_fn_proto_call(
    ast: &Ast,
    m_name: &str,
    args: &[ExprId],
    fn_params: &[Param],
) -> bool {
    let fn_params_len = fn_params.len();
    match m_name {
        "call" => true,
        "apply" => match args.len() {
            0 | 1 => true,
            2 => {
                matches!(
                    ast.exprs.get(args[1].0 as usize),
                    Some(Expr::Array(_)) | Some(Expr::Null)
                ) || matches!(
                    ast.exprs.get(args[1].0 as usize),
                    Some(Expr::Ident(n)) if n == "undefined"
                )
            }
            _ => false,
        },
        // Knife 4 — a this-using target spends one more param slot
        // than the source wrote: its thisArg becomes the leading
        // partial (see the `bind` arm), so the surplus test has to
        // count it. Getting this wrong desynchronizes the two callers
        // — the collector would leave a receiver unwrapped for a form
        // the desugar then declines to swallow.
        "bind" => {
            let implicit_recv = usize::from(fn_params.first().is_some_and(|p| p.name == "__this"));
            args.len().saturating_sub(1) + implicit_recv <= fn_params_len
        }
        _ => false,
    }
}

pub(crate) fn run(ast: &mut Ast) {
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = s
        {
            if name.starts_with("__bound_") || name.starts_with("__bind_create_") {
                continue;
            }
            fn_sigs.insert(name.clone(), (params.clone(), return_type.clone()));
        }
    }
    if fn_sigs.is_empty() {
        return;
    }

    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut id_counter: u32 = 0;

    let n = ast.exprs.len();
    for i in 0..n {
        let (callee_eid, args_clone) = match &ast.exprs[i] {
            Expr::Call { callee, args } => (*callee, args.clone()),
            _ => continue,
        };
        let (obj_eid, m_name) = match ast.exprs.get(callee_eid.0 as usize) {
            Some(Expr::Member { obj, name }) => (*obj, name.clone()),
            _ => continue,
        };
        if m_name != "bind" && m_name != "call" && m_name != "apply" {
            continue;
        }
        let fn_name = match ast.exprs.get(obj_eid.0 as usize) {
            Some(Expr::Ident(n)) => n.clone(),
            _ => continue,
        };
        let Some((mut fn_params, mut fn_ret)) = fn_sigs.get(&fn_name).cloned() else {
            continue;
        };
        if fn_params.first().is_some_and(|p| p.name == "__env") {
            continue;
        }

        // Knife 3c — `C.g.call(recv, …)` arrives here as
        // `Ident("__sm_C__g").call(…)` (the static-member rewrite runs
        // inside desugar_classes, before this pass). The mono has no
        // receiver channel at all, but a this-using static minted a
        // `__smany_` twin (knife 3a): devirtualize the rebind to a
        // direct twin call, thisArg in the leading `__this` slot. A
        // twin-less static keeps the historic drop (this-free bodies
        // run identically; mint residues keep mono behavior).
        //
        // Resolved BEFORE the swallow test, and the retargeted
        // signature is what everything downstream reads: the twin has
        // one more param than the mono, and `.bind` spends a param
        // slot per partial, so testing the mono's arity here would
        // answer for the wrong function.
        let mut fn_name = fn_name;
        let mut retargeted = false;
        if let Some(twin) = retarget_static_twin(&fn_name, &fn_params, |t| fn_sigs.contains_key(t))
            && let Some((twin_params, twin_ret)) = fn_sigs.get(&twin).cloned()
        {
            fn_name = twin;
            fn_params = twin_params;
            fn_ret = twin_ret;
            retargeted = true;
        }
        if !swallows_fn_proto_call(ast, &m_name, &args_clone, &fn_params) {
            continue;
        }

        // RFC 20260804-fn-this-channel knife 1 — a this-using target
        // (bind_this_param, which runs earlier in the pipeline, gave it
        // a `__this: any` leading param) receives the thisArg in that
        // slot per §22.2.3.{3,4}; only a this-free target keeps the
        // historic thisArg drop (its body never reads the receiver).
        // A retargeted static reads this-using off the twin's own
        // leading `__this`.
        let this_using = fn_params.first().is_some_and(|p| p.name == "__this");
        // `.bind` synthesizes its own callee Ident from `fn_name`, so
        // it needs no rewritten callee here — minting one would leave
        // an orphan node in the arena for whole-arena walks to find.
        let callee_target = if retargeted && m_name != "bind" {
            ast.add_expr(Expr::Ident(fn_name.clone()))
        } else {
            obj_eid
        };

        match m_name.as_str() {
            "call" => {
                // `f.call()` is a zero-arg invocation with thisArg
                // undefined — a this-free target makes the empty form
                // `f()`; a this-using one gets an explicit `undefined`
                // receiver (strict mode skips ToObject, §10.2.1.2).
                let mut new_args: Vec<ExprId> = Vec::new();
                if this_using {
                    let recv = match args_clone.first() {
                        Some(&t) => t,
                        None => ast.add_expr(Expr::Ident("undefined".into())),
                    };
                    new_args.push(recv);
                }
                new_args.extend(args_clone.iter().skip(1).copied());
                ast.exprs[i] = Expr::Call {
                    callee: callee_target,
                    args: new_args,
                };
            }
            "apply" => {
                // §22.2.3.1: argArray absent / undefined / null means
                // "call with no arguments" — so `f.apply()` /
                // `f.apply(t)` / `f.apply(t, undefined|null)` are all
                // the bare invocation. Only a present, non-nullish
                // argArray demands the array-literal unpack (dynamic
                // arrays stay unsupported here).
                let lit_args: Vec<ExprId> = match args_clone.len() {
                    0 | 1 => Vec::new(),
                    2 => match ast.exprs.get(args_clone[1].0 as usize) {
                        Some(Expr::Array(els)) => els.clone(),
                        Some(Expr::Null) => Vec::new(),
                        Some(Expr::Ident(n)) if n == "undefined" => Vec::new(),
                        _ => continue,
                    },
                    _ => continue,
                };
                let mut new_args: Vec<ExprId> = Vec::new();
                if this_using {
                    let recv = match args_clone.first() {
                        Some(&t) => t,
                        None => ast.add_expr(Expr::Ident("undefined".into())),
                    };
                    new_args.push(recv);
                }
                new_args.extend(lit_args);
                ast.exprs[i] = Expr::Call {
                    callee: callee_target,
                    args: new_args,
                };
            }
            "bind" => {
                // `f.bind()` / `f.bind(t)` bind no partial args; a
                // partial list covering every param leaves a zero-param
                // bound fn (`__cls()->R` interns an empty param list).
                // Only MORE partials than params keeps the reject —
                // dropping surplus args would drop their evaluation.
                let mut partial_args: Vec<ExprId> = args_clone.iter().skip(1).copied().collect();
                // Knife 4 (RFC 20260804-fn-this-channel) — binding a
                // thisArg IS partially applying the receiver slot:
                // bind_this_param made it param 0, so the thisArg just
                // leads the partial list and rides the factory's
                // capture env like any other bound argument. `f.bind()`
                // on a this-using target binds an explicit undefined
                // (§22.2.3.2 takes thisArg however it comes, and strict
                // mode skips ToObject per §10.2.1.2).
                if this_using {
                    let recv = match args_clone.first() {
                        Some(&t) => t,
                        None => ast.add_expr(Expr::Ident("undefined".into())),
                    };
                    partial_args.insert(0, recv);
                }
                if partial_args.len() > fn_params.len() {
                    continue;
                }
                let id = id_counter;
                id_counter += 1;
                synth_bind(
                    ast,
                    i,
                    id,
                    &fn_name,
                    &fn_params,
                    &fn_ret,
                    partial_args,
                    &mut new_decls,
                );
            }
            _ => unreachable!(),
        }
    }

    ast.stmts.extend(new_decls);
}

/// The `.bind` rewrite's synthesis half: mint the `__bound_<f>_<id>`
/// wrapper + `__bind_create_<f>_<id>` factory pair and replace the
/// call at arena slot `i` with the factory invocation over the
/// partial args (see module doc).
#[allow(clippy::too_many_arguments)]
fn synth_bind(
    ast: &mut Ast,
    i: usize,
    id: u32,
    fn_name: &str,
    fn_params: &[Param],
    fn_ret: &Option<String>,
    partial_args: Vec<ExprId>,
    new_decls: &mut Vec<Stmt>,
) {
    let partial_count = partial_args.len();
    let bound_name = format!("__bound_{}_{}", fn_name, id);
    let factory_name = format!("__bind_create_{}_{}", fn_name, id);

    let cap_names: Vec<String> = (0..partial_count)
        .map(|k| format!("__bp_{}_{}", id, k))
        .collect();
    let remaining_count = fn_params.len() - partial_count;
    let rem_names: Vec<String> = (0..remaining_count)
        .map(|k| format!("__br_{}_{}", id, k))
        .collect();

    let env_ann = format!("__env({})", cap_names.join("|"));
    let mut bound_params: Vec<Param> = Vec::with_capacity(remaining_count + 1);
    bound_params.push(Param {
        name: "__env".into(),
        type_ann: Some(env_ann),
        default: None,
        is_rest: false,
    });
    for k in 0..remaining_count {
        let src_param = &fn_params[partial_count + k];
        bound_params.push(Param {
            name: rem_names[k].clone(),
            type_ann: src_param.type_ann.clone(),
            default: None,
            is_rest: false,
        });
    }
    let callee_ident_id = ast.add_expr(Expr::Ident(fn_name.to_string()));
    let mut call_args: Vec<ExprId> = Vec::with_capacity(fn_params.len());
    for cn in &cap_names {
        call_args.push(ast.add_expr(Expr::Ident(cn.clone())));
    }
    for rn in &rem_names {
        call_args.push(ast.add_expr(Expr::Ident(rn.clone())));
    }
    let call_id = ast.add_expr(Expr::Call {
        callee: callee_ident_id,
        args: call_args,
    });
    let bound_body = vec![Stmt::Return(Some(call_id))];
    // An unannotated source fn's return type is inferred
    // later — `any` is the only annotation the factory can
    // write that agrees with whatever that inference says
    // (`void` rejected every value-returning unannotated fn).
    let ret_type_str = fn_ret.clone().unwrap_or_else(|| "any".to_string());
    let bound_decl = Stmt::FnDecl {
        name: bound_name.clone(),
        type_params: Vec::new(),
        params: bound_params,
        // Mirrors the factory's `->R` (any when the source
        // is unannotated) so the wrapper's own return
        // boundary boxes the forwarded value — an inferred
        // Number returned raw through a `__cls(..)->any`
        // slot reads back as a garbage tag.
        return_type: Some(ret_type_str.clone()),
        body: bound_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    };

    let mut factory_params: Vec<Param> = Vec::with_capacity(partial_count);
    for k in 0..partial_count {
        let src_param = &fn_params[k];
        factory_params.push(Param {
            name: cap_names[k].clone(),
            type_ann: src_param.type_ann.clone(),
            default: None,
            is_rest: false,
        });
    }
    let rem_tys: Vec<String> = (partial_count..fn_params.len())
        .map(|k| {
            fn_params[k]
                .type_ann
                .clone()
                .unwrap_or_else(|| "any".to_string())
        })
        .collect();
    let factory_ret = format!("__cls({})->{}", rem_tys.join("|"), ret_type_str);
    let closure_expr_id = ast.add_expr(Expr::Closure {
        fn_name: bound_name,
        captures: cap_names.clone(),
    });
    let factory_body = vec![Stmt::Return(Some(closure_expr_id))];
    new_decls.push(Stmt::FnDecl {
        name: factory_name.clone(),
        type_params: Vec::new(),
        params: factory_params,
        return_type: Some(factory_ret),
        body: factory_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    new_decls.push(bound_decl);

    let new_callee = ast.add_expr(Expr::Ident(factory_name));
    ast.exprs[i] = Expr::Call {
        callee: new_callee,
        args: partial_args,
    };
}
