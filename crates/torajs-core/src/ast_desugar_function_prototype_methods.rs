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
//! `.bind` is NOT one of them. It used to be: the pass synthesized a
//! `__bound_<id>` wrapper plus a `__bind_create_<id>` factory and
//! replaced the call with the factory invocation. That product is a
//! second bound-function implementation, and it answered three things
//! the spec reads off the TARGET at bind time — it had no
//! [[Construct]] (`new f.bind(null)()` threw where bun constructs),
//! and its `length` / `name` came from the declaration the compiler
//! saw rather than from `Get(Target, "length"/"name")`, so a
//! `defineProperty`'d length was invisible. The runtime kernel
//! (`torajs_anyvalue::method_bind`) gets all three right for the any
//! lane already, so every `.bind` goes there now and this pass leaves
//! the member call alone.
//!
//! Skips fn sources whose first param is `__env` (closure-shaped
//! sources need env-forwarding — the runtime kernel owns those).

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Whether this pass will rewrite `f.<m_name>(args)` for a plain
/// top-FnDecl `f` declaring `fn_params`. The single source of
/// truth the collector's receiver-wrap axis consults: a form this
/// pass swallows becomes a direct call (receiver must NOT wrap, or
/// the Ident match below would stop seeing it); a form it leaves —
/// a dynamic argArray, or any `.bind` at all — keeps the member
/// call, whose fn-name receiver then rides the wrapped closure lane.
pub(crate) fn swallows_fn_proto_call(
    ast: &Ast,
    m_name: &str,
    args: &[ExprId],
    _fn_params: &[Param],
) -> bool {
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
        // `bind` is never swallowed — see the module doc.
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
            fn_sigs.insert(name.clone(), (params.clone(), return_type.clone()));
        }
    }
    if fn_sigs.is_empty() {
        return;
    }

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
        if m_name != "call" && m_name != "apply" {
            continue;
        }
        let fn_name = match ast.exprs.get(obj_eid.0 as usize) {
            Some(Expr::Ident(n)) => n.clone(),
            _ => continue,
        };
        let Some((fn_params, _fn_ret)) = fn_sigs.get(&fn_name).cloned() else {
            continue;
        };
        if fn_params.first().is_some_and(|p| p.name == "__env") {
            continue;
        }
        if !swallows_fn_proto_call(ast, &m_name, &args_clone, &fn_params) {
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
        if !this_using && let Some(rest) = fn_name.strip_prefix("__sm_") {
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
            _ => unreachable!(),
        }
    }
}
