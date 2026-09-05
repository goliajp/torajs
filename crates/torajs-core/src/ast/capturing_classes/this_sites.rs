//! Where does a body say `this`?
//!
//! Two questions the lane asks of a class body before it lowers it:
//! every `this` node under a statement list (the receiver-promotion
//! registrations the hoist's remap moves), and whether a single
//! expression says `this` anywhere (a static field initialiser that
//! does must wrap into the `.call(K)` form rather than inline bare at
//! the store).

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::{Ast, Expr, ExprId, Stmt};

/// Every `this` node in `body`. A nested `function` expression binds
/// its own, so descending into one over-answers — the safe direction
/// for both callers: a registration cleared one time too many only
/// sends that `this` down the same channel a function expression would
/// have used anyway, and the hoist's remap (what makes this
/// `pub(super)`) moves only sites still registered under the name it is
/// renaming. It does not descend into a nested class body, so a class
/// inside one of these keeps its own registrations either way.
pub(in crate::ast) fn this_sites(ast: &Ast, body: &[Stmt]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut pending: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = pending.pop() {
        for e in stmt_exprs(s) {
            this_sites_in_expr(ast, e, &mut out);
        }
        pending.extend(stmt_children_ref(s));
    }
    out
}

/// Does this expression say `this` anywhere under it — arrow bodies
/// included? Asked of a static field's initializer, which runs at
/// class-evaluation time with `this` bound to the class object: a
/// `this`-free one has nothing to lose by being inlined at the store,
/// while one that reads `this` must wrap into the
/// `(function () { … }).call(K)` form the emit mints (394-05) — bare
/// at the store it would silently pick up the ENCLOSING receiver.
pub(in crate::ast) fn expr_says_this(ast: &Ast, root: ExprId) -> bool {
    let mut sites = Vec::new();
    this_sites_in_expr(ast, root, &mut sites);
    !sites.is_empty()
}

fn this_sites_in_expr(ast: &Ast, root: ExprId, out: &mut Vec<ExprId>) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::This => out.push(eid),
            // An arrow's body is a statement list, not a child expr.
            Expr::ArrowFn { body, .. } => out.extend(this_sites(ast, body)),
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
}
