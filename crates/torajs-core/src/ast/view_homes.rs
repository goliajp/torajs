//! Which expressions are consumed where they are produced — the
//! census a temporary split product consults (the "Temporaries"
//! section of [`super::let_owned_elems`]).
//!
//! `s.split(sep)` answers an array of substring views that may only
//! live in the split block that owns their storage. A bound product
//! is ruled on by its binding's window; a product that is never
//! bound has no window, so the question is asked of its position:
//!
//! - a binding's init — the binding lane decides;
//! - a discarded statement value — nothing reads it later;
//! - the object of a member or index READ — `.length`, `[i]`,
//!   `.join(..)`, the receiver of any method that does not store an
//!   owned value into the array.
//!
//! Everything else — an argument, a literal element, a returned,
//! yielded or assigned value, a conditional's branch, a spread — is
//! on its way to a receiver typed by its own annotation, and the
//! lowering hands the product on as owned strings there. A written
//! target is not a read, and neither is the receiver of `push` and
//! the other owned-storing mutators. Keyed by the ExprId the lowering
//! holds for the call.

use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Stmt};

/// The methods that store an owned value into their receiver.
pub(super) fn stores_owned(method: &str) -> bool {
    matches!(
        method,
        "push" | "unshift" | "splice" | "fill" | "copyWithin"
    )
}

/// Record `eid`, through the `as` wrappers a position can carry.
pub(super) fn mark_home(ast: &Ast, eid: ExprId, homes: &mut HashSet<ExprId>) {
    homes.insert(eid);
    if let Expr::As { expr, .. } = ast.get_expr(eid) {
        mark_home(ast, *expr, homes);
    }
}

/// The statement-level homes: a binding's init and a discarded
/// statement value.
pub(super) fn mark_stmt_homes(ast: &Ast, s: &Stmt, homes: &mut HashSet<ExprId>) {
    match s {
        Stmt::Expr(e) => mark_home(ast, *e, homes),
        Stmt::LetDecl { init, .. } => mark_home(ast, *init, homes),
        _ => {}
    }
}

/// The object of every member or index read, over the whole arena.
pub(super) fn mark_read_objects(ast: &Ast, homes: &mut HashSet<ExprId>) {
    let mut written: HashSet<ExprId> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                written.insert(*target);
            }
            Expr::Delete { expr } => {
                written.insert(*expr);
            }
            _ => {}
        }
    }
    for (i, e) in ast.exprs.iter().enumerate() {
        if written.contains(&ExprId(i as u32)) {
            continue;
        }
        match e {
            Expr::Member { obj, name } if !stores_owned(name) => mark_home(ast, *obj, homes),
            Expr::Index { obj, .. } => mark_home(ast, *obj, homes),
            _ => {}
        }
    }
}
