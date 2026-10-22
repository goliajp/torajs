//! The type annotation a literal receiver or argument spells.

use super::super::{Ast, Expr, ExprId};

/* Resolve a literal expression's type ann string.
 * - `Array(els)`      → infer `T[]` from els[0]'s shape (literal
 *                       receiver path — `[1,2,3].map(x => ...)`).
 *                       Empty literal can't infer an element type;
 *                       skipped. Only homogeneous-typed literals
 *                       matter here since the existing `T[]` infra
 *                       requires homogeneous elements.
 * - `String`          → "string"
 * - `Number`          → "number"
 * Anything more exotic falls through unchanged (caller relies on
 * an explicit annotation upstream). */
pub(in crate::ast) fn infer_lit_ann(ast: &Ast, eid: ExprId) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::Array(els) if !els.is_empty() => {
            /* Recurse on EVERY element — the `T[]` claim is only
             * sound for a homogeneous literal (the header comment
             * always said so, but the code sampled els[0] only, so
             * `[1, Symbol()].forEach(props => ...)` typed the
             * callback param "number" and the Any elem coerce threw
             * ToNumber(Symbol) at the use site — rotation 162
             * staging appeared case). A mixed or unrecognized
             * literal answers None and the callback params stay
             * `any`. */
            let first = infer_lit_ann(ast, els[0])?;
            for &e in &els[1..] {
                if infer_lit_ann(ast, e).as_deref() != Some(first.as_str()) {
                    return None;
                }
            }
            Some(format!("{first}[]"))
        }
        _ => None,
    }
}
