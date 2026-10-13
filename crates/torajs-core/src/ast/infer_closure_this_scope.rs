//! `__this`-scoped contextual typing for a class field's arrow — split
//! from sibling [`super::infer_closure_lets`], whose program-wide
//! assignment walk answers every other target shape.
//!
//! Kept apart because the two answer the same question from opposite
//! directions: that walk reads a table keyed by bare name, and `__this`
//! is the one name a flat table cannot hold — every synthesized class
//! factory, constructor and method has its own.

use super::infer_closure_lets::{field_ann_of, lifted_closure_name};
use super::{Ast, Expr, ExprId, Stmt};

/// Is this assignment target reached through `__this`?
pub(super) fn is_this_rooted(ast: &Ast, target: ExprId) -> bool {
    match ast.get_expr(target) {
        Expr::Ident(n) => n == "__this",
        Expr::Member { obj, .. } | Expr::Index { obj, .. } => is_this_rooted(ast, *obj),
        _ => false,
    }
}

/// `__this` names a different object in every function: each
/// synthesized class factory, constructor and method takes one,
/// annotated with its own class. The table the program-wide assignment
/// walk reads ([`super::infer_closure_lets::seed_assign_hints`]) is flat
/// and keyed by bare name, so one entry served the whole program and
/// the class declared LAST won every lookup.
///
/// A field initializer in any earlier class then resolved its receiver
/// to the wrong class. `class Inner { f: (a: number) => number = (a) =>
/// a * 3 }` followed by a second class — an EMPTY one was enough —
/// lifted its arrow with an `any` param while the call site passed an
/// unboxed number, so the body ran with a garbage argument and the call
/// answered 0 instead of throwing or refusing. When that other class
/// happened to declare a field of the same name it was worse still: the
/// arrow took THAT field's type, so `Inner.f` was lifted `(a: string)
/// -> string`. Declaring the second class FIRST hid all of it.
///
/// Answered per function instead, where the annotation is unambiguous.
/// The walk mirrors the statement recursion in
/// [`super::infer_closure_lets::collect_let_anns`], and an assignment it
/// does not reach simply carries no hint — never another class's.
pub(super) fn seed_this_assign_hints(
    ast: &Ast,
    body: &[Stmt],
    this_ann: &str,
    closure_hints: &mut std::collections::HashMap<String, String>,
) {
    for s in body {
        match s {
            Stmt::Expr(e) => {
                let Expr::Assign { target, value } = ast.get_expr(*e) else {
                    continue;
                };
                let Some(fn_name) = lifted_closure_name(ast, *value) else {
                    continue;
                };
                let Expr::Member { obj, name } = ast.get_expr(*target) else {
                    continue;
                };
                if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "__this") {
                    continue;
                }
                if let Some(ann) = field_ann_of(ast, this_ann, name) {
                    closure_hints.insert(fn_name, ann);
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                seed_this_assign_hints(ast, stmts, this_ann, closure_hints)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                seed_this_assign_hints(
                    ast,
                    std::slice::from_ref(then_branch.as_ref()),
                    this_ann,
                    closure_hints,
                );
                if let Some(eb) = else_branch {
                    seed_this_assign_hints(
                        ast,
                        std::slice::from_ref(eb.as_ref()),
                        this_ann,
                        closure_hints,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                seed_this_assign_hints(
                    ast,
                    std::slice::from_ref(body.as_ref()),
                    this_ann,
                    closure_hints,
                );
            }
            Stmt::For { body, .. } => {
                seed_this_assign_hints(
                    ast,
                    std::slice::from_ref(body.as_ref()),
                    this_ann,
                    closure_hints,
                );
            }
            _ => {}
        }
    }
}

/// The class `__this` names at each call reached through it, answered
/// per function for the same reason [`seed_this_assign_hints`] is: the
/// flat table holds one `__this` for the whole program.
///
/// A generator's locals live on its `__this` (`__this.fs.push(cb)`),
/// and so do an awaiting async function's. With a second such function
/// anywhere in the program, the receiver of every `push` but the last
/// function's resolved to the wrong class, the stored arrow kept an
/// `any` param, and the call through the array handed it an unboxed
/// number. Calls outside every function with a `__this` of its own are
/// absent and keep the table's answer.
pub(super) fn this_anns_by_call(ast: &Ast) -> std::collections::HashMap<ExprId, String> {
    use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
    type Out = std::collections::HashMap<ExprId, String>;
    fn walk_expr(ast: &Ast, e: ExprId, ann: &str, out: &mut Out) {
        if matches!(ast.get_expr(e), Expr::Call { .. }) {
            out.insert(e, ann.to_string());
        }
        for c in expr_children(ast, e) {
            walk_expr(ast, c, ann, out);
        }
    }
    fn walk_stmt(ast: &Ast, s: &Stmt, ann: &str, out: &mut Out) {
        for e in stmt_exprs(s) {
            walk_expr(ast, e, ann, out);
        }
        for c in stmt_children_ref(s) {
            walk_stmt(ast, c, ann, out);
        }
    }
    let mut out = Out::new();
    for s in &ast.stmts {
        let Stmt::FnDecl { params, body, .. } = s else {
            continue;
        };
        let this_ann = params.iter().find(|p| p.name == "__this");
        let Some(ann) = this_ann.and_then(|p| p.type_ann.as_deref()) else {
            continue;
        };
        for st in body {
            walk_stmt(ast, st, ann, &mut out);
        }
    }
    out
}
