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
            Stmt::Labeled { body, .. } => {
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

/// Walk `body` for annotated `let` / `const` declarations, filling two
/// tables from the same visit:
///
/// - `out` — binding name → annotation, the receiver table the HOF arms
///   read (`const a: number[] = …; a.map(x => …)`).
/// - `closure_hints` — for a binding whose init IS a closure, the
///   LIFTED closure's name → that same annotation. `const g: (n: number)
///   => number = (n) => n` contextually types the arrow's params from
///   its target type, the way TS does; without it the unannotated param
///   keeps its `any` default while the call site dispatches through the
///   annotation's signature, and the two ABIs disagree.
///
/// One walk, two outputs, on purpose: a second copy of this control-flow
/// recursion is a copy that drifts.
pub(super) fn collect_let_anns(
    ast: &Ast,
    body: &[Stmt],
    out: &mut std::collections::HashMap<String, String>,
    closure_hints: &mut std::collections::HashMap<String, String>,
) {
    for s in body {
        match s {
            Stmt::LetDecl {
                name,
                type_ann: Some(ann),
                init,
                ..
            } => {
                out.insert(name.clone(), ann.clone());
                // Both post-lift shapes, as at the call-arg positions:
                // `Expr::Closure` when the arrow captured something,
                // a bare ident at the lifted FnDecl when it did not.
                let lifted = match ast.get_expr(*init) {
                    Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                    Expr::Ident(n) if n.starts_with("__closure_") => Some(n.clone()),
                    _ => None,
                };
                if let Some(fn_name) = lifted {
                    closure_hints.insert(fn_name, ann.clone());
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                collect_let_anns(ast, stmts, out, closure_hints)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_anns(
                    ast,
                    std::slice::from_ref(then_branch.as_ref()),
                    out,
                    closure_hints,
                );
                if let Some(eb) = else_branch {
                    collect_let_anns(ast, std::slice::from_ref(eb.as_ref()), out, closure_hints);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_anns(ast, std::slice::from_ref(body.as_ref()), out, closure_hints);
            }
            Stmt::Labeled { body, .. } => {
                collect_let_anns(ast, std::slice::from_ref(body.as_ref()), out, closure_hints);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_anns(ast, std::slice::from_ref(i.as_ref()), out, closure_hints);
                }
                collect_let_anns(ast, std::slice::from_ref(body.as_ref()), out, closure_hints);
            }
            _ => {}
        }
    }
}
