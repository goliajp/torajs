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
