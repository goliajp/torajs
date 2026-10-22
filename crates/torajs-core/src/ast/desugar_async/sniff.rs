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
//! - every return agrees on one type the static sniff can name without
//!   reading a parameter's annotation: a caller holding the function as
//!   `any` can pass anything, and `return s.length` on `s: string`
//!   handed a number settles with undefined, not a number;
//! - that type is a primitive: a returned object with a callable
//!   `then` is adopted rather than settled with (§27.2.1.3.2 steps
//!   9-13), and only a primitive is never an object (step 7).

use super::super::{Ast, Stmt, body_always_terminates, infer_return_ann};

pub(super) fn sniffed_inner(ast: &Ast, body: &[Stmt]) -> Option<String> {
    if !body_always_terminates(body) {
        return None;
    }
    let t = infer_return_ann(&ast.exprs, body, &[], &Default::default())?;
    matches!(t.as_str(), "number" | "string" | "boolean").then_some(t)
}
