//! Promise `.then(cb)` / `.catch(cb)` cb-param inference — the
//! sibling arm of [`crate::ast::infer_closure_params`], extracted so
//! the main call-site loop stays within the file-size hard limit.
//!
//! The narrow contract: given a `.then` / `.catch` receiver
//! expression, walk two shapes back to the source Promise's inner
//! type so the callback's un-annotated user param can be seeded
//! before `desugar_lifted_closure_fn` defaults it to `any`:
//!
//! - `Promise.resolve(v)` / `Promise.reject(v)` mints — the inner is
//!   `v`'s static type (literal or annotated ident).
//! - Chained `.then(prev_cb)` / `.catch(prev_cb)` — the inner is
//!   `prev_cb`'s return annotation. Explicit or already-sniffable
//!   returns come from `fn_ret_anns` (populated once up front from
//!   the pre-mutation FnDecls). When `prev_cb`'s body sniff failed
//!   because a user param was un-annotated (`n => n`), fall through
//!   to a recursive resolve of the receiver's chain, seed the param,
//!   and re-sniff — the second try then succeeds and the ret ann
//!   propagates. Recursion is bounded by the AST shape.

use super::infer_closure_params::infer_lit_ann;
use super::{Ast, Expr, ExprId, Stmt, infer_return_ann};

pub(super) fn resolve_promise_inner_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
    fn_ret_anns: &std::collections::HashMap<String, Option<String>>,
) -> Option<String> {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    match name.as_str() {
        "resolve" | "reject" => {
            if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Promise") {
                return None;
            }
            args.first()
                .and_then(|a| resolve_arg_ann(ast, *a, all_anns))
        }
        "then" | "catch" => {
            let src = || source_inner_ann(ast, *obj, all_anns, fn_ret_anns);
            let ok_leg = |cb| handler_ret_ann(ast, cb, Leg::Fulfil, src, all_anns, fn_ret_anns);
            let err_leg = |cb| handler_ret_ann(ast, cb, Leg::Reject, src, all_anns, fn_ret_anns);
            match (name.as_str(), &args[..]) {
                // §27.2.5.4: the derived promise settles from whichever
                // handler runs, so its value is either handler's return;
                // `.catch(onErr)` passes a fulfilled source's value through
                ("then", [on_ok, on_err, ..]) => join_legs(&ok_leg(*on_ok)?, &err_leg(*on_err)?),
                ("catch", [on_err, ..]) => join_legs(&src()?, &err_leg(*on_err)?),
                (_, [on_ok, ..]) => ok_leg(*on_ok),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Is `eid` a `.then(onOk, onErr)` / `.catch(onErr)` call, whose value
/// [`join_legs`] answers?
fn settles_from_two_legs(ast: &Ast, eid: ExprId) -> bool {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return false;
    };
    matches!(
        ast.get_expr(*callee),
        Expr::Member { name, .. } if (name == "then" && args.len() >= 2) || (name == "catch" && !args.is_empty())
    )
}

/// The value annotation of the promise a `.then` / `.catch` is called
/// on: a chain or mint [`resolve_promise_inner_ann`] walks, or a binding
/// declared `Promise<T>`.
fn source_inner_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
    fn_ret_anns: &std::collections::HashMap<String, Option<String>>,
) -> Option<String> {
    if let Expr::Ident(n) = ast.get_expr(eid) {
        let ann = all_anns.get(n)?;
        let inner = ann.strip_prefix("Promise<")?.strip_suffix('>')?;
        return Some(inner.to_string());
    }
    resolve_promise_inner_ann(ast, eid, all_anns, fn_ret_anns)
}

/// Which settlement a handler argument runs on.
#[derive(Clone, Copy)]
enum Leg {
    Fulfil,
    Reject,
}

/// The return annotation of the handler `cb` on `leg`: a closure's own
/// (explicit or sniffed up front), else the sniff rerun with its
/// parameter seeded (the source's value on the fulfil leg, `any` for a
/// reason) when an un-annotated parameter blocked the up-front one;
/// a top-level function's declared or sniffed return. Identity chains
/// (`n => n`) propagate through here without needing a mid-pass
/// rebuild.
///
/// §27.2.5.4 PerformPromiseThen steps 3-4 replace a handler that is not
/// callable: on the fulfil leg the source's value passes through, on
/// the reject leg the reason does, so that leg settles no value
/// (`never`).
fn handler_ret_ann(
    ast: &Ast,
    cb: ExprId,
    leg: Leg,
    src: impl FnOnce() -> Option<String>,
    all_anns: &std::collections::HashMap<String, String>,
    fn_ret_anns: &std::collections::HashMap<String, Option<String>>,
) -> Option<String> {
    let fn_name = match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name.clone(),
        Expr::Ident(n) if n.starts_with("__closure_") => n.clone(),
        Expr::Null => return pass_through(leg, src),
        Expr::Ident(n) if n == "undefined" => return pass_through(leg, src),
        // a binding of the name shadows the top-level function
        Expr::Ident(n) if !all_anns.contains_key(n) => return fn_ret_anns.get(n)?.clone(),
        _ => return None,
    };
    if let Some(Some(r)) = fn_ret_anns.get(&fn_name) {
        return Some(r.clone());
    }
    let seed = match leg {
        Leg::Fulfil => src()?,
        Leg::Reject => "any".to_string(),
    };
    sniff_body_with_param_seed(ast, &fn_name, &seed)
}

fn pass_through(leg: Leg, src: impl FnOnce() -> Option<String>) -> Option<String> {
    match leg {
        Leg::Fulfil => src(),
        Leg::Reject => Some("never".to_string()),
    }
}

/// The value annotation of a promise settled from one of two legs: each
/// leg's value is its return adopted (a returned promise's own value)
/// with a void return answering undefined, and a throw-only leg settles
/// nothing. Legs that agree keep their one type. The checker types the
/// derived promise the same way (`two_leg_result` in
/// `check_type_of_call_promise_then`).
///
/// A join that may settle undefined into a pointer-shaped `T` answers
/// nothing: the then kernel converts an undefined settled value into a
/// raw pointer parameter as NULL, so a handler seeded `T | undefined`
/// would read undefined as null. Its parameter keeps the any lane until
/// that conversion keeps undefined.
fn join_legs(a: &str, b: &str) -> Option<String> {
    let value = |t: &str| -> Option<String> {
        match t {
            "never" => None,
            "void" => Some("undefined".to_string()),
            t => Some(
                match t.strip_prefix("Promise<").and_then(|r| r.strip_suffix('>')) {
                    Some(inner) => inner.to_string(),
                    None => t.to_string(),
                },
            ),
        }
    };
    let (a, b) = (value(a), value(b));
    let joined = match (&a, &b) {
        (Some(a), Some(b)) if a == b => a.clone(),
        (Some(a), Some(b)) => crate::parser::union_combine(&[a, b]),
        (Some(v), None) | (None, Some(v)) => v.clone(),
        (None, None) => "undefined".to_string(),
    };
    let settles_undefined = [a, b]
        .iter()
        .flatten()
        .any(|v| v == "undefined" || v.starts_with("__nullable("));
    let pointer_inner = joined
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|t| !matches!(t, "number" | "boolean"));
    (!(settles_undefined && pointer_inner)).then_some(joined)
}

/// Rerun the return-ann sniff for a lifted closure body with the
/// first user param seeded to `param_inner_ann` — used when the
/// up-front sniff failed only because the param carried no ann.
fn sniff_body_with_param_seed(ast: &Ast, fn_name: &str, param_inner_ann: &str) -> Option<String> {
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
            && name == fn_name
        {
            let mut synth = params.clone();
            let user_start = if synth.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            if let Some(p) = synth.get_mut(user_start)
                && p.type_ann.is_none()
            {
                p.type_ann = Some(param_inner_ann.to_string());
            }
            return infer_return_ann(&ast.exprs, body, &synth, &std::collections::HashMap::new());
        }
    }
    None
}

/// Best-effort type annotation for an expression flowing into a
/// `Promise.resolve/reject(arg)` mint — the arg's static shape.
fn resolve_arg_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Ident(n) => all_anns.get(n).cloned(),
        _ => infer_lit_ann(ast, eid),
    }
}

/// Seed the `.then` / `.catch` callback's un-annotated user params
/// from the source promise's inner type.
///
/// Without this the param would default to `any` in
/// `desugar_lifted_closure_fn`; the pthunk pre-scan then skips the
/// callback (its param-face gate accepts only `None | Some("number")`)
/// and the runtime dispatcher hands the raw i64 slot to a cb whose Any
/// param cannot decode an F64 bit pattern — the user sees the raw bits
/// as a huge integer.
///
/// A `T | null` source seeds `any`, not `T | null`. This pass reads
/// the DECLARATION — it runs long before anything knows about
/// narrowing — so `s = "hi"; Promise.resolve(s).then((v) => v + "!")`
/// seeded `v: string | null` and the body was refused on a program
/// whose value is a string throughout. The receiver, which does see
/// the narrow, says `Promise<string>`; `handler_param_admits` already
/// reconciles the two SIGNATURES, but the body is checked against
/// whatever this pass wrote.
///
/// `any` is what a nullable rides anyway (`rides_any_lane`), and it is
/// the honest seed here: this pass cannot know whether the value
/// reaching the handler is null. Unseeded it would default to `any`
/// too — the seed exists to keep an F64 source off the raw-i64 slot,
/// which a nullable source never was. An un-narrowed null then reaches
/// the body as a null and fails there, which is where bun fails. A
/// handler that writes its own annotation keeps it (the seed only ever
/// fills an absent one).
/// Only the FULFILLED handler receives the promise's inner value.
/// `.catch(cb)` and `.then(onOk, onErr)`'s second argument receive the
/// rejection reason, which §27.2.5.4 leaves to whatever was thrown —
/// unrelated to the inner type. Seeding those with it made a rejection
/// handler read its reason through the wrong width: `.catch(e =>
/// typeof e)` on a `Promise<number>` rejected with a string answered
/// `"number"`, because the boxed reason reaches an `REPR_F64`
/// parameter and `settle_param` answers NaN for a non-numeric box.
/// `any` is the honest seed there, for the same reason the nullable
/// case above takes it.
fn reason_position(method: &str, arg_idx: usize) -> bool {
    method == "catch" || arg_idx >= 1
}

#[allow(clippy::too_many_arguments)]
pub(super) fn seed_then_catch_params(
    ast: &Ast,
    obj: ExprId,
    method: &str,
    all_anns: &std::collections::HashMap<String, String>,
    fn_ret_anns: &std::collections::HashMap<String, Option<String>>,
    closure_args: &[(usize, String)],
    fn_user_param_count: &std::collections::HashMap<String, usize>,
    param_only_updates: &mut std::collections::HashMap<String, Vec<String>>,
) {
    let Some(inner_ann) = resolve_promise_inner_ann(ast, obj, all_anns, fn_ret_anns) else {
        return;
    };
    // a union of two handlers' returns is no declaration a narrow could
    // have refined, so it seeds as written
    let seed = if inner_ann.starts_with("__nullable(") && !settles_from_two_legs(ast, obj) {
        "any".to_string()
    } else {
        inner_ann
    };
    for (arg_idx, fn_name) in closure_args {
        let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
        if p >= 1 {
            let s = if reason_position(method, *arg_idx) {
                "any".to_string()
            } else {
                seed.clone()
            };
            param_only_updates.insert(fn_name.clone(), vec![s; p]);
        }
    }
}
