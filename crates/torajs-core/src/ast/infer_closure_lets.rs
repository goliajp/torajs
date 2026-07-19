//! Let-declaration annotation collectors feeding
//! `infer_anonymous_closure_params`'s receiver table (split from
//! infer_closure_params.rs when the mapset forEach seed arm pushed
//! the file past the 500-line limit): `collect_let_anns` gathers
//! explicit `let x: T` annotations, `collect_let_init_anns` infers
//! them from literal / `new Map<K, V>()` init shapes.

use super::{Ast, Expr, ExprId, Stmt};

/// Walk `body` collecting `let name = <init>` shapes where the init is a
/// literal whose type can be inferred (number / string / boolean / array
/// of any of those). Populates `out` with `name → "T[]" / "T"` strings,
/// matching the format used by `infer_anonymous_closure_params`'s
/// `infer_lit_ann` helper. Used so unannotated top-level lets still feed
/// the closure-param inference pass.
pub(super) fn collect_let_init_anns(
    ast: &Ast,
    body: &[Stmt],
    out: &mut std::collections::HashMap<String, String>,
) {
    fn ann_of(ast: &Ast, eid: ExprId) -> Option<String> {
        match ast.get_expr(eid) {
            Expr::Number(_) => Some("number".into()),
            Expr::String(_) => Some("string".into()),
            Expr::Bool(_) => Some("boolean".into()),
            Expr::Array(els) if !els.is_empty() => {
                ann_of(ast, els[0]).map(|inner| format!("{inner}[]"))
            }
            // `new Map<string, number>()` / `new Set<number>()` — the
            // explicit instantiation spelling carried on the New node
            // is the binding's ann (`Map<string|number>`, the same
            // flat form parse_type_ann produces).
            Expr::New {
                class_name,
                type_args,
                ..
            } if matches!(class_name.as_str(), "Map" | "Set") && !type_args.is_empty() => {
                Some(format!("{class_name}<{}>", type_args.join("|")))
            }
            _ => None,
        }
    }
    for s in body {
        match s {
            Stmt::LetDecl {
                name,
                type_ann: None,
                init,
                ..
            } => {
                if let Some(ann) = ann_of(ast, *init) {
                    out.insert(name.clone(), ann);
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_let_init_anns(ast, stmts, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_init_anns(ast, std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_let_init_anns(ast, std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_init_anns(ast, std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

pub(super) fn collect_let_anns(body: &[Stmt], out: &mut std::collections::HashMap<String, String>) {
    for s in body {
        match s {
            Stmt::LetDecl {
                name,
                type_ann: Some(ann),
                ..
            } => {
                out.insert(name.clone(), ann.clone());
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_let_anns(stmts, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_anns(std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_let_anns(std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_anns(std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}
