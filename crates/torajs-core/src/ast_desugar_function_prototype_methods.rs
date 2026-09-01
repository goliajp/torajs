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
//!   factory `__bind_create_<id>(captures...) -> __cls(rem_tys)->(R)`
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
use crate::ast_desugar_function_prototype_methods_synth::synth_bind;

/// Whether this pass will rewrite `f.<m_name>(args)` for a plain
/// top-FnDecl `f` declaring `fn_params`. The single source of
/// truth the collector's receiver-wrap axis consults: a form this
/// pass swallows becomes a direct call (receiver must NOT wrap, or
/// the Ident match below would stop seeing it); a form it leaves —
/// dynamic argArray, surplus bind partials, an arguments-touching
/// bind target — keeps the member call, whose fn-name receiver then
/// rides the wrapped closure lane. `target_fn` is the FnDecl the
/// call really dispatches to (a closure BINDING receiver resolves
/// through its alias first).
pub(crate) fn swallows_fn_proto_call(
    ast: &Ast,
    m_name: &str,
    args: &[ExprId],
    fn_params: &[Param],
    target_fn: &str,
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
            // RFC 20260808 knife 2 — the static synth lane forwards a
            // FIXED arg list (declared params), so an
            // arguments-touching body would see the declared arity
            // whatever the real call wrote. The runtime bind kernel's
            // bound_entry concatenates partials with the live call's
            // argv (real argc end to end) — route there by keeping
            // the member call. Wider than the collectors' admission
            // is the safe direction: a body they refuse stays loud on
            // either lane.
            if fn_body_touches_arguments(ast, target_fn) {
                return false;
            }
            let implicit_recv = usize::from(fn_params.first().is_some_and(|p| p.name == "__this"));
            args.len().saturating_sub(1) + implicit_recv <= fn_params_len
        }
        _ => false,
    }
}

/// True when the named top-level FnDecl's body touches `arguments`
/// in any form (indexing, spread, `.length`, aliasing). The bind
/// arm's kernel-lane routing predicate — see
/// [`swallows_fn_proto_call`].
pub(crate) fn fn_body_touches_arguments(ast: &Ast, fn_name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, body, .. }
            if name == fn_name
                && (crate::ast::body_has_non_length_arguments_touch(ast, body)
                    || crate::ast::body_has_arguments_length(ast, body)))
    })
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
    let closure_aliases = collect_closure_aliases(ast, &fn_sigs);

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
        let mut sig = fn_sigs.get(&fn_name).cloned();
        let mut bind_target_is_binding = false;
        if sig.is_none() && m_name == "bind" {
            sig = closure_binding_sig(&fn_name, &closure_aliases, &fn_sigs);
            bind_target_is_binding = sig.is_some();
        }
        let Some((fn_params, fn_ret)) = sig else {
            continue;
        };
        if fn_params.first().is_some_and(|p| p.name == "__env") {
            continue;
        }
        // The arguments-touch probe reads the REAL FnDecl body — a
        // closure binding resolves through its lifted alias.
        let target_fn = closure_aliases
            .get(&fn_name)
            .cloned()
            .unwrap_or_else(|| fn_name.clone());
        if !swallows_fn_proto_call(ast, &m_name, &args_clone, &fn_params, &target_fn) {
            continue;
        }

        // RFC 20260804-fn-this-channel knife 1 — a this-using target
        // (bind_this_param, which runs earlier in the pipeline, gave it
        // a `__this: any` leading param) receives the thisArg in that
        // slot per §22.2.3.{3,4}; only a this-free target keeps the
        // historic thisArg drop (its body never reads the receiver).
        let mut this_using = fn_params.first().is_some_and(|p| p.name == "__this");
        // Knife 3c — `C.g.call(recv, …)` arrives here as
        // `Ident("__sm_C__g").call(…)` (the static-member rewrite runs
        // inside desugar_classes, before this pass). The mono has no
        // receiver channel at all, but a this-using static minted a
        // `__smany_` twin (knife 3a): devirtualize the rebind to a
        // direct twin call, thisArg in the leading `__this` slot. A
        // twin-less static keeps the historic drop (this-free bodies
        // run identically; mint residues keep mono behavior).
        let mut callee_target = obj_eid;
        if !this_using
            && m_name != "bind"
            && let Some(rest) = fn_name.strip_prefix("__sm_")
        {
            let twin = format!("__smany_{rest}");
            if fn_sigs.contains_key(&twin) {
                callee_target = ast.add_expr(Expr::Ident(twin));
                this_using = true;
            }
        }

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
                // bound fn (`__cls()->(R)` interns an empty param list).
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
                    bind_target_is_binding.then_some(fn_name.as_str()),
                );
            }
            _ => unreachable!(),
        }
    }

    ast.stmts.extend(new_decls);
}

/// fnexpr-bind knife — `var f = function () {…}` arrives here
/// (post-lift) as a LetDecl whose init is the lifted closure node.
/// Resolve the binding name to that FnDecl for the BIND arm only
/// (narrow surface: `.call` / `.apply` on a closure binding keep
/// their current lanes).
fn collect_closure_aliases(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>)>,
) -> HashMap<String, String> {
    let mut closure_aliases: HashMap<String, String> = HashMap::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        let Stmt::LetDecl {
            name,
            init,
            // An ANNOTATED binding types as its annotation, not as the
            // closure signature — `const h: any = (x, y) => …` reaches
            // the factory call as Any and the checker rejects the
            // precise `__cls` param. Those stay on the any-method bind
            // kernel they already ride.
            type_ann: None,
            ..
        } = s
        else {
            continue;
        };
        // Post-lift a capture-less fn-expr init is a
        // `Closure { fn_name, captures: [] }` node (the bare-Ident
        // specialization happens later); a capturing one keeps its
        // env and stays on the loud reject via the `__env` gate.
        let cn = match ast.get_expr(*init) {
            Expr::Ident(cn) => cn,
            Expr::Closure { fn_name, captures } if captures.is_empty() => fn_name,
            _ => continue,
        };
        if cn.starts_with("__closure_") && fn_sigs.contains_key(cn) {
            closure_aliases.insert(name.clone(), cn.clone());
        }
    }
    closure_aliases
}

/// The bind arm's receiver signature for a closure BINDING: the
/// lifted FnDecl's params MINUS the `__env` slot (the synth wrapper
/// forwards through the binding name — a bare-name closure call,
/// which carries the env itself — so the wrapper never spells
/// `__env` and the lifted arity gate must not see it either).
/// Untyped params forward as `any`, same as the fn-ctor factory.
fn closure_binding_sig(
    fn_name: &str,
    closure_aliases: &HashMap<String, String>,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>)>,
) -> Option<(Vec<Param>, Option<String>)> {
    let cn = closure_aliases.get(fn_name)?;
    let (cp, cr) = fn_sigs.get(cn)?;
    // An unannotated return leaves the synth lane no truthful `->R`
    // to write on the `__bt` capture: the source's real convention is
    // its INFERRED type (raw slot), while the only writable fallback
    // (`->any`) makes the wrapper's inner call read that raw slot as
    // a NaN-box — `D.bind(null, 21)()` answered a garbage tag, the
    // zero-partial form decoded the box as a cell. The named-FnDecl
    // arm has no such lie (its wrapper calls through the NAME, the
    // checker infers the ret, and the wrapper's own `->any` boundary
    // boxes); a binding without a ret annotation instead declines
    // here and rides the runtime bind kernel, whose boxed adapter
    // owns both conventions end to end.
    cr.as_ref()?;
    let stripped: Vec<Param> = cp
        .iter()
        .filter(|p| p.name != "__env")
        .map(|p| Param {
            type_ann: p.type_ann.clone().or_else(|| Some("any".into())),
            ..p.clone()
        })
        .collect();
    Some((stripped, cr.clone()))
}
