//! ES ToPropertyKey (§7.1.19) compile-time fold for literal keys —
//! chunk 745. One spelling shared by the parser's object-literal
//! numeric-key arm and the checker/SSA struct-index lanes, so
//! `{ 0: v }`, `g[0]`, and `g["0"]` all agree on the field name "0".

use super::{Ast, Expr, ExprId};

/// ES Number-to-property-key spelling: integral finite values print
/// as integers (`0` not `0.0`), everything else takes the shortest
/// float form — matching bun's serialization.
pub fn number_prop_key(n: f64) -> String {
    if n.is_finite() && n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A compile-time literal index expression folded to its property
/// key (`g[0]` → "0", `g["0"]` → "0"); `None` for dynamic indices.
/// Identifier-shaped string literals never reach the Index shape
/// (the parser's V3-18 wedge folds them to Member), so hits here
/// are numeric keys and non-identifier string keys.
pub fn literal_prop_key(ast: &Ast, index: ExprId) -> Option<String> {
    match ast.get_expr(index) {
        Expr::Number(n) => Some(number_prop_key(*n)),
        Expr::String(s) => Some(s.to_string_lossy_owned()),
        _ => None,
    }
}
