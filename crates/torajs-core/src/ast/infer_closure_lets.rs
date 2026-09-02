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
    declared_rets: &std::collections::HashMap<String, String>,
    out: &mut std::collections::HashMap<String, String>,
) {
    fn ann_of(
        ast: &Ast,
        eid: ExprId,
        declared_rets: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        match ast.get_expr(eid) {
            Expr::Number(_) => Some("number".into()),
            Expr::String(_) => Some("string".into()),
            Expr::Bool(_) => Some("boolean".into()),
            Expr::Array(els) if !els.is_empty() => {
                ann_of(ast, els[0], declared_rets).map(|inner| format!("{inner}[]"))
            }
            // A call to a function that declares what it returns —
            // including the factory a `new C()` has become by now, so
            // `const c = new C()` binds `C` and a method call on one
            // of its fields has a receiver type. Only a *declared*
            // return counts: a sniffed one is a guess, and this table
            // is read as if it were an annotation.
            Expr::Call { callee, .. } => match ast.get_expr(*callee) {
                Expr::Ident(fname) => declared_rets.get(fname).cloned(),
                _ => None,
            },
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
                if let Some(ann) = ann_of(ast, *init, declared_rets) {
                    out.insert(name.clone(), ann);
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                collect_let_init_anns(ast, stmts, declared_rets, out)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_init_anns(
                    ast,
                    std::slice::from_ref(then_branch.as_ref()),
                    declared_rets,
                    out,
                );
                if let Some(eb) = else_branch {
                    collect_let_init_anns(
                        ast,
                        std::slice::from_ref(eb.as_ref()),
                        declared_rets,
                        out,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), declared_rets, out);
            }
            Stmt::Labeled { body, .. } => {
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), declared_rets, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_init_anns(
                        ast,
                        std::slice::from_ref(i.as_ref()),
                        declared_rets,
                        out,
                    );
                }
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), declared_rets, out);
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
                if let Some(fn_name) = lifted_closure_name(ast, *init) {
                    closure_hints.insert(fn_name, ann.clone());
                }
                seed_container_field_hints(ast, ann, *init, closure_hints);
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

/// The lifted closure an initializer names, in either post-lift shape:
/// `Expr::Closure` when the arrow captured something, a bare ident at
/// the lifted FnDecl when it did not.
pub(super) fn lifted_closure_name(ast: &Ast, init: ExprId) -> Option<String> {
    match ast.get_expr(init) {
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        Expr::Ident(n) if n.starts_with("__closure_") => Some(n.clone()),
        _ => None,
    }
}

/// The same contextual typing, carried into an object or array
/// literal: `const o: { f: (a: number) => number } = { f: (a) => … }`
/// and `const fs: ((n: number) => number)[] = [(n) => …]`. Nested
/// literals recurse, so a field whose type is another object type
/// carries its own fields' types inward too.
///
/// Only the direct binding was carrying its annotation inward, so an
/// arrow written straight into a field or an element kept the default
/// its params get with no context — which is `string`. The call site
/// still dispatched through the declared signature, so the two sides
/// disagreed about what a parameter even is: `o.f(3)` read the number
/// as a Str pointer and `typeof a` answered "string" inside a field
/// declared `(a: number) => number`.
///
/// Field annotations come from a named `TypeDecl` or an inline object
/// type; an array annotation is its element type with the `[]` taken
/// off. Anything that does not resolve to a fn type is skipped, and a
/// param the author annotated still wins downstream — the applier is
/// the same one the direct-binding hint goes through.
pub(super) fn seed_container_field_hints(
    ast: &Ast,
    ann: &str,
    init: ExprId,
    closure_hints: &mut std::collections::HashMap<String, String>,
) {
    match ast.get_expr(init) {
        Expr::ObjectLit { fields } => {
            let Some(field_anns) = resolve_object_field_anns(ast, ann) else {
                return;
            };
            for (fname, fexpr) in fields {
                let Some(fann) = fname.as_str().and_then(|f| field_anns.get(f)) else {
                    continue;
                };
                match lifted_closure_name(ast, *fexpr) {
                    Some(fn_name) => {
                        closure_hints.insert(fn_name, fann.clone());
                    }
                    None => seed_container_field_hints(ast, fann, *fexpr, closure_hints),
                }
            }
        }
        Expr::Array(elems) => {
            let Some(elem_ann) = ann.trim().strip_suffix("[]") else {
                return;
            };
            let elem_ann = elem_ann.trim();
            // `((n: number) => number)[]` — the element type keeps the
            // parens the array suffix needed.
            let elem_ann = elem_ann
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .unwrap_or(elem_ann);
            for e in elems {
                match lifted_closure_name(ast, *e) {
                    Some(fn_name) => {
                        closure_hints.insert(fn_name, elem_ann.to_string());
                    }
                    None => seed_container_field_hints(ast, elem_ann, *e, closure_hints),
                }
            }
        }
        _ => {}
    }
}

/// The declared annotation of field `fname` on an object typed `ann`.
/// The one lookup behind both the container walk above and the
/// receiver resolution in [`super::infer_closure_params`], so a field
/// type reads the same whether the arrow is written into it or handed
/// to a method on it.
pub(super) fn field_ann_of(ast: &Ast, ann: &str, fname: &str) -> Option<String> {
    resolve_object_field_anns(ast, ann)?.get(fname).cloned()
}

/// Field name → annotation for an object type named by `ann`, whether
/// it is a `TypeDecl` name or an inline object type.
fn resolve_object_field_anns(
    ast: &Ast,
    ann: &str,
) -> Option<std::collections::HashMap<String, String>> {
    let ann = ann.trim();
    if let Some(fields) = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann) {
        return Some(fields);
    }
    ast.stmts.iter().find_map(|s| match s {
        Stmt::TypeDecl { name, fields, .. } if name == ann => {
            Some(fields.iter().cloned().collect())
        }
        _ => None,
    })
}

/// The assignment counterpart: writing an arrow into an already-typed
/// place types it, the same way initializing that place does.
///
/// A binding, a field and an element all say what they hold, and only
/// the declaration position was reading it — so the second line of
///
///     let g: (n: number) => number = (n) => n;
///     g = (n) => n + 1;
///
/// left the arrow contextless while the call still dispatched through
/// the declared signature, and `g(3)` SIGSEGVed. `fs[0] = cb` on a
/// declared array answered -562949953421311 and `o.f = cb` on a
/// declared field crashed the same way the rebind did.
///
/// Walks the expression arena rather than the statement tree: an
/// assignment is an expression and can sit anywhere one can, and this
/// question does not care where.
pub(super) fn seed_assign_hints(
    ast: &Ast,
    all_anns: &std::collections::HashMap<String, String>,
    closure_hints: &mut std::collections::HashMap<String, String>,
) {
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let Some(fn_name) = lifted_closure_name(ast, *value) else {
            continue;
        };
        // `__this` is not a program-wide name — see
        // [`seed_this_assign_hints`], which answers for it per function.
        if super::infer_closure_this_scope::is_this_rooted(ast, *target) {
            continue;
        }
        if let Some(ann) = assign_target_ann(ast, *target, all_anns) {
            closure_hints.insert(fn_name, ann);
        }
    }
}

/// What an assignment target says it holds: a binding's own
/// annotation, a field's declared type, or an array's element type.
fn assign_target_ann(
    ast: &Ast,
    target: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match ast.get_expr(target) {
        Expr::Ident(n) => all_anns.get(n).cloned(),
        Expr::Member { obj, name } => {
            let obj_ann = super::infer_closure_params::resolve_receiver_ann(ast, *obj, all_anns)?;
            field_ann_of(ast, &obj_ann, name)
        }
        Expr::Index { obj, .. } => {
            let obj_ann = super::infer_closure_params::resolve_receiver_ann(ast, *obj, all_anns)?;
            Some(obj_ann.trim().strip_suffix("[]")?.to_string())
        }
        _ => None,
    }
}

/// The `return` counterpart of the container walk: a fn whose declared
/// return type is a fn type contextually types an arrow it returns.
///
/// `function make(): (n: number) => number { return (n) => n + 1 }` —
/// the arrow is written with no annotation of its own, and without
/// this it takes the same contextless `string` default that a field
/// or an element used to, so `make()(3)` answered 3.
///
/// Nested `FnDecl` bodies are not descended into: their returns
/// belong to their own declared type, and each is reached by the
/// caller's own walk over `ast.stmts`.
pub(super) fn seed_return_hints(
    ast: &Ast,
    body: &[Stmt],
    ret_ann: &str,
    closure_hints: &mut std::collections::HashMap<String, String>,
) {
    for s in body {
        match s {
            Stmt::Return(Some(e)) => {
                if let Some(fn_name) = lifted_closure_name(ast, *e) {
                    closure_hints.insert(fn_name, ret_ann.to_string());
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                seed_return_hints(ast, stmts, ret_ann, closure_hints)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                seed_return_hints(
                    ast,
                    std::slice::from_ref(then_branch.as_ref()),
                    ret_ann,
                    closure_hints,
                );
                if let Some(eb) = else_branch {
                    seed_return_hints(
                        ast,
                        std::slice::from_ref(eb.as_ref()),
                        ret_ann,
                        closure_hints,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                seed_return_hints(
                    ast,
                    std::slice::from_ref(body.as_ref()),
                    ret_ann,
                    closure_hints,
                );
            }
            Stmt::For { body, .. } => {
                seed_return_hints(
                    ast,
                    std::slice::from_ref(body.as_ref()),
                    ret_ann,
                    closure_hints,
                );
            }
            _ => {}
        }
    }
}
