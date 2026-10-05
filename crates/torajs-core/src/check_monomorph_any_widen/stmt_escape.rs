//! The statement-position half of the generic-fn value-escape ledger
//! (`Checker::fn_escape_widen_sites`).

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Checker;
use crate::ssa_lower_generics_monomorph::Generics;

/// A generic fn read as a bare expression STATEMENT (`f;`) escapes as
/// a value exactly like a `const g = f` init: the read materializes
/// the fn's ADDRESS, and a generic original is a checker template
/// that never lowers — the read died with "unknown ident" where bun
/// evaluates and discards it. The compile-time `Function(...)`
/// rewrite lands in this shape whenever the call sits in statement
/// position (`Function('a', 'a', 'return;');`, the §20.2.1.1
/// duplicate-parameter family), because an unannotated parameter is
/// an implicit generic.
///
/// Same channel as `check_stmt_let_decl`'s L3b ③ arm, minus the face
/// widening: a discarded read binds nothing, so only the retarget is
/// wanted. Recording here rather than at the statement check keeps
/// the escape ledger in one module.
pub(crate) fn record_stmt_expr_escapes(c: &mut Checker, ast: &Ast, generics: &Generics) {
    let mut sites: Vec<(ExprId, String)> = Vec::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        let Stmt::Expr(eid) = s else { return };
        let Expr::Ident(n) = ast.get_expr(*eid) else {
            return;
        };
        if generics.contains_key(n.as_str()) {
            sites.push((*eid, n.clone()));
        }
    });
    for (eid, name) in sites {
        c.fn_escape_widen_sites.entry(eid).or_insert(name);
    }
}
