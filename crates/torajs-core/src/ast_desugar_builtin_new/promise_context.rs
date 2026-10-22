//! Which promise a `new Promise(executor)` makes, read from where the
//! expression sits.
//!
//! The desugar in `promise.rs` turns the expression into a call of a
//! helper and picks the helper by this reading; closure-param inference
//! later types the executor's `resolve` by the same reading. Both ask
//! here, so the promise and its `resolve` never disagree about `T`.
//!
//! `T` comes from, nearest last: the written `Promise<T>` return of the
//! function returning the expression, an annotated binding it
//! initializes, an assertion `as Promise<T>`, a `new Promise<T>` type
//! argument (kept in `call_type_args` by the rewrite), and the helper
//! the call already names.

use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// The helper a `new Promise(executor)` calls when nothing names `T`;
/// it returns `any`.
pub(crate) const EXECUTOR_HELPER: &str = "__promise_from_executor";

/// The `T`s a helper returning `Promise<T>` is made for. A primitive is
/// never adopted as a thenable (§27.2.1.3.2 step 9), and a handler
/// typed with one reads the settled value whatever produced it; an
/// object or array element type does not (its handler reads the
/// widths its own annotation names, not the ones the value was built
/// with).
const SETTLED: [&str; 3] = ["number", "string", "boolean"];

/// `t` when a helper returning `Promise<t>` can be made for it.
pub(crate) fn settled(t: &str) -> Option<&'static str> {
    SETTLED.iter().copied().find(|s| *s == t)
}

/// The helper whose promise settles with `t`; `None` names the `any` one.
pub(crate) fn helper_name(t: Option<&str>) -> String {
    match t {
        Some(t) => format!("{EXECUTOR_HELPER}__{t}"),
        None => EXECUTOR_HELPER.to_string(),
    }
}

/// `Some(None)` for the `any` helper, `Some(Some(t))` for the one
/// returning `Promise<t>`, `None` for any other name.
pub(crate) fn executor_helper(name: &str) -> Option<Option<&'static str>> {
    let rest = name.strip_prefix(EXECUTOR_HELPER)?;
    if rest.is_empty() {
        return Some(None);
    }
    settled(rest.strip_prefix("__")?).map(Some)
}

/// `T` of an annotation spelled exactly `Promise<T>`; `None` for a
/// union of promises, a subclass, or anything else whose settled value
/// the spelling does not name.
pub(crate) fn promise_inner(ann: &str) -> Option<String> {
    let inner = ann.trim().strip_prefix("Promise<")?.strip_suffix('>')?;
    // the `>` stripped must close the `Promise<` itself:
    // `Promise<A> | Promise<B>` ends in `>` too
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then(|| inner.trim().to_string())
}

/// Each executor-helper call whose `T` is known and passes `keep` → `T`.
pub(crate) fn executor_contexts(ast: &Ast, keep: impl Fn(&str) -> bool) -> HashMap<ExprId, String> {
    let mut out = HashMap::new();
    let note = |out: &mut HashMap<ExprId, String>, e: ExprId, ann: &str| {
        if let Some(t) = promise_inner(ann)
            && helper_call(ast, e).is_some()
            && keep(&t)
        {
            out.insert(e, t);
        }
    };
    // every function's own returns, under its own return type
    let mut stack: Vec<(&Stmt, Option<&str>)> = ast.stmts.iter().map(|s| (s, None)).collect();
    while let Some((st, rt)) = stack.pop() {
        match st {
            Stmt::FnDecl {
                return_type, body, ..
            } => {
                let rt = return_type.as_deref();
                stack.extend(body.iter().map(|s| (s, rt)));
            }
            Stmt::Return(Some(e)) => {
                if let Some(rt) = rt {
                    note(&mut out, *e, rt);
                }
            }
            Stmt::LetDecl {
                type_ann: Some(ann),
                init,
                ..
            } => note(&mut out, *init, ann),
            _ => {}
        }
        stack.extend(stmt_children_ref(st).into_iter().map(|s| (s, rt)));
    }
    // an assertion, then the type argument, then the helper's own `T`
    // are the nearest contexts, so they are read last
    for e in &ast.exprs {
        if let Expr::As { expr, ty_ann } = e {
            note(&mut out, *expr, ty_ann);
        }
    }
    for (call, args) in &ast.call_type_args {
        if let [t] = args.as_slice() {
            note(&mut out, *call, &format!("Promise<{t}>"));
        }
    }
    for i in 0..ast.exprs.len() {
        let e = ExprId(i as u32);
        if let Some(Some(t)) = helper_call(ast, e) {
            out.insert(e, t.to_string());
        }
    }
    out
}

/// What [`executor_helper`] says of the callee, when `e` is a call of
/// an executor helper.
pub(crate) fn helper_call(ast: &Ast, e: ExprId) -> Option<Option<&'static str>> {
    let Expr::Call { callee, .. } = ast.get_expr(e) else {
        return None;
    };
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    executor_helper(n)
}
