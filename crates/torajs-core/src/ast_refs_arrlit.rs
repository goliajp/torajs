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
/// (`[]` has no statically certain element type), any non-literal
/// element, or mixed element shapes. Integral and fractional Number
/// literals unify to the wide `f64[]` slot (storing f64 bits in an
/// i64 slot reads back garbage; the reverse widens losslessly).
pub(crate) fn arrlit_literal_elem_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::Array(elems) = ast.get_expr(init) else {
        return None;
    };
    if elems.is_empty() {
        return None;
    }
    let mut elem_ann: Option<&str> = None;
    for e in elems {
        let ann = match ast.get_expr(*e) {
            Expr::Number(n) => {
                if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
                    "f64"
                } else {
                    "number"
                }
            }
            Expr::String(_) => "string",
            Expr::Bool(_) => "boolean",
            _ => return None,
        };
        elem_ann = Some(match (elem_ann, ann) {
            (None, a) => a,
            (Some(prev), a) if prev == a => prev,
            (Some("number"), "f64") | (Some("f64"), "number") => "f64",
            _ => return None,
        });
    }
    Some(format!("{}[]", elem_ann?))
}
