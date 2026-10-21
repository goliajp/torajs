//! The check of a lifted closure body's clone.

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Checker;

/// 551-03 — check a lifted body's clone the way its original was
/// checked: at the construction site, with the captures bound
/// (`check_closure`), not as a top-level FnDecl walk — that walk zips
/// the `__env` head against the first user type and leaves the real
/// params unbound. The clone joins `stmts` first (`closure_sig`
/// resolves the body by name there); the captures are the original
/// site's, which the admit gate proved resolve at this level.
pub(super) fn check_lifted_spec(c: &mut Checker, owned_ast: &Ast, fn_name: &str, spec_decl: &Stmt) {
    let mono_name = match spec_decl {
        Stmt::FnDecl { name, .. } => name.clone(),
        _ => unreachable!("widened spec is a FnDecl"),
    };
    let (site, captures) = owned_ast
        .exprs
        .iter()
        .enumerate()
        .find_map(|(i, e)| match e {
            crate::ast::Expr::Closure {
                fn_name: f,
                captures,
            } if f == fn_name => Some((ExprId(i as u32), captures.clone())),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("any-widen: lifted closure `{fn_name}` has no construction site")
        });
    let _ = crate::check_type_of_fn::check_closure(c, owned_ast, site, &mono_name, &captures);
}
