//! Cluster-`values` follow-up (rotation 253) — the canonical `T[]`
//! spelling for an all-literal-element Array init on an un-annotated
//! top-level binding (`var values = [2, 1, 3];`, the dominant test262
//! prelude shape — read from fn bodies and parameter-default guards).
//!
//! Both the checker's pass_2 registration and the lowerer's K.3b slot
//! inference resolve the SAME string through their existing annotation
//! pipelines (the `__inlobj(...)` precedent in `ast_refs`), so the two
//! slots cannot drift — and the interned Arr layout unifies with an
//! equivalent written annotation.

use crate::ast::{Ast, Expr, ExprId};

/// `None` — keeping the binding main-local — for an empty literal
/// (`[]` has no statically certain element type, at any depth), any
/// non-literal element, or mixed element shapes. Integral and
/// fractional Number literals unify to the wide `f64[]` slot at any
/// shared nesting depth (storing f64 bits in an i64 slot reads back
/// garbage; the reverse widens losslessly). Nested Array literals
/// recurse (`[[1, 2], [3]]` → `number[][]` — the test262 matrix
/// prelude shape).
pub(crate) fn arrlit_literal_elem_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::Array(elems) = ast.get_expr(init) else {
        return None;
    };
    if elems.is_empty() {
        return None;
    }
    let mut elem_ann: Option<String> = None;
    for e in elems {
        let ann: String = match ast.get_expr(*e) {
            Expr::Number(n) => if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
                "f64"
            } else {
                "number"
            }
            .to_string(),
            Expr::String(_) => "string".to_string(),
            Expr::Bool(_) => "boolean".to_string(),
            Expr::Array(_) => arrlit_literal_elem_ann(ast, *e)?,
            _ => return None,
        };
        elem_ann = Some(match elem_ann {
            None => ann,
            Some(prev) if prev == ann => prev,
            Some(prev) => unify_number_width(&prev, &ann)?,
        });
    }
    Some(format!("{}[]", elem_ann?))
}

/// `number`/`f64` unify to the wide spelling when their nesting
/// depths agree (`number[]` + `f64[]` → `f64[]`); anything else is a
/// genuine shape mix and answers `None`.
fn unify_number_width(a: &str, b: &str) -> Option<String> {
    let base_a = a.trim_end_matches("[]");
    let base_b = b.trim_end_matches("[]");
    let depth = (a.len() - base_a.len()) / 2;
    if a.len() - base_a.len() != b.len() - base_b.len() {
        return None;
    }
    if matches!((base_a, base_b), ("number", "f64") | ("f64", "number")) {
        return Some(format!("f64{}", "[]".repeat(depth)));
    }
    None
}
