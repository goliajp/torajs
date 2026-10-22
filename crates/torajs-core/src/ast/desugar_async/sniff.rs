//! The inner type of an async function that writes no return type.
//!
//! Without an annotation the promise was declared `Promise<any>`, so
//! `m().then((v) => …)` saw `v: any` even for `async function m() {
//! return 1; }`. TS infers `Promise<number>` there, and so does this,
//! under conditions that keep the answer exact:
//!
//! - every path ends in a `return` or a `throw` — a path off the end
//!   settles with undefined (§10.2.1.4 step 11), which the sniffed type
//!   would not include;
//! - every return agrees on one type the static sniff can name with
//!   each parameter read as `any`: a caller holding the function as
//!   `any` can pass anything, and `return s.length` on `s: string`
//!   handed a number settles with undefined, not a number, while
//!   `const n: number = x` on `x: any` holds `x` as it is;
//! - that type is a primitive: a returned object with a callable
//!   `then` is adopted rather than settled with (§27.2.1.3.2 steps
//!   9-13), and only a primitive is never an object (step 7).

use super::super::{Ast, Param, Stmt, body_always_terminates, infer_return_ann};

/// The written return type, else the sniffed primitive, else `any`.
///
/// A function whose sniffed type is the one it declares joins
/// `Ast::async_proved_returns`: its returns settle that type whatever
/// the checker makes of a binding they read (a `var` the checker holds
/// as `any`), so the check round keeps its `Promise<T>`.
pub(super) fn declared_or_sniffed(
    ast: &mut Ast,
    name: &str,
    written: Option<String>,
    params: &[Param],
    body: &[Stmt],
) -> String {
    let sniffed = sniffed_inner(ast, params, body);
    if let Some(t) = &sniffed
        && written
            .as_ref()
            .is_none_or(|w| *w == format!("Promise<{t}>"))
    {
        ast.async_proved_returns.insert(name.to_string());
    }
    written.or(sniffed).unwrap_or_else(|| "any".into())
}

fn sniffed_inner(ast: &Ast, params: &[Param], body: &[Stmt]) -> Option<String> {
    if !body_always_terminates(body) {
        return None;
    }
    let as_any: Vec<Param> = (params.iter())
        .map(|p| Param {
            type_ann: Some("any".into()),
            ..p.clone()
        })
        .collect();
    let t = infer_return_ann(&ast.exprs, body, &as_any, &Default::default())?;
    matches!(t.as_str(), "number" | "string" | "boolean").then_some(t)
}
