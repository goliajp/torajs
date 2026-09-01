//! Static `Object.fromEntries` fold (chunk 690, T-09).
//!
//! `Object.fromEntries([["a", e1], ["b", e2]])` with a pairs ARRAY
//! LITERAL whose keys are all string literals is exactly the object
//! literal `{ a: e1, b: e2 }` (ES §20.1.2.7 walks the entries in
//! order; the array literal fixes that order statically), so it
//! folds to `Expr::ObjectLit` before typecheck and rides the
//! anonymous-struct lanes end to end — checker schema inference,
//! ssa-lower, AOT — with no new runtime surface. Value exprs move
//! verbatim (their evaluation order — pair by pair — matches the
//! object literal's field order); keys are effect-free string
//! literals.
//!
//! Shapes left on the existing loud reject ("unsupported member
//! call shape: fromEntries"): dynamic entries (a variable /
//! Map.entries() — needs the dynobj construction loop, recorded),
//! non-literal or duplicate keys, non-pair elements, empty entries,
//! and the trailing-args form (S309 keeps trailing evaluation).

use super::{Ast, Expr, ExprId};

pub fn fold_fromentries(ast: &mut Ast) {
    for i in 0..ast.exprs.len() {
        let Some(fields) = try_fold(ast, i) else {
            continue;
        };
        ast.exprs[i] = Expr::ObjectLit { fields };
    }
}

/// Read-only shape match: `Object.fromEntries(<array literal of
/// [string-literal, value] pairs>)` with unique keys.
fn try_fold(ast: &Ast, i: usize) -> Option<Vec<(String, ExprId)>> {
    let Expr::Call { callee, args } = &ast.exprs[i] else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if name != "fromEntries" {
        return None;
    }
    if !matches!(ast.get_expr(*obj), Expr::Ident(ns) if ns == "Object") {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    let Expr::Array(entries) = ast.get_expr(args[0]) else {
        return None;
    };
    if entries.is_empty() {
        return None;
    }
    let mut fields: Vec<(String, ExprId)> = Vec::with_capacity(entries.len());
    for &e in entries {
        let Expr::Array(pair) = ast.get_expr(e) else {
            return None;
        };
        let [k, v] = pair.as_slice() else {
            return None;
        };
        let Expr::String(key) = ast.get_expr(*k) else {
            return None;
        };
        if fields.iter().any(|(f, _)| key == f) {
            return None;
        }
        fields.push((key.to_string_lossy_owned(), *v));
    }
    Some(fields)
}
