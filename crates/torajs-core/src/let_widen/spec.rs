//! The type half inside a specialization.
//!
//! The monomorphizer checks a clone of a function after the program's
//! own run has settled the widen set: a parameter it widened to `any`
//! (`f(s: string)` called with an `any`) makes `i = i + s.length`
//! store an `any` into `let i = 0` in the clone only. The lowering
//! finds that binding from the final types and gives it an `any` slot,
//! so the checker has to type its reads `any` too, or a typed consumer
//! of `i` is handed a box. So a clone is checked again while the type
//! half over it finds bindings the set does not hold yet.

use std::collections::HashSet;

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Checker;

/// Check the specialization `decl` with `check`, again for as long as
/// that grows the widen set.
pub(crate) fn check_spec(
    c: &mut Checker,
    ast: &Ast,
    decl: &Stmt,
    mut check: impl FnMut(&mut Checker, &Ast),
) {
    check(c, ast);
    loop {
        let found: HashSet<ExprId> =
            super::collect_cross_type_widen_inits_in(ast, decl, &c.expr_types);
        if found.is_subset(&c.cross_type_widened) {
            return;
        }
        c.cross_type_widened.extend(found);
        check(c, ast);
    }
}

/// [`check_spec`] for a declaration checked as a statement.
pub(crate) fn check_spec_stmt(c: &mut Checker, ast: &Ast, decl: &Stmt) {
    check_spec(c, ast, decl, |c, a| c.check_stmt(a, decl));
}
