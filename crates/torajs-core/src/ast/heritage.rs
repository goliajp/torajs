//! Class-heritage helpers (RFC 20260815-heritage-exprid).
//!
//! A `ClassDecl`'s `parent` is an `ExprId` per §15.7 (the heritage is a
//! LeftHandSideExpression). Every static consumer — class_index,
//! class_parents, super rewrites, hoist admission — keys on a NAME, so
//! they all read the heritage through `parent_ident_name`: the answer is
//! `Some(name)` exactly when the heritage is a bare identifier, which is
//! the only shape those static paths can (and did) handle. A non-Ident
//! heritage answers `None` and is routed to the value-shaped-parent lane
//! instead (RFC knife 2).

use super::{Ast, Expr, ExprId};

impl Ast {
    /// The statically-known parent-class name of a heritage expression:
    /// `Some(name)` iff it is a bare `Expr::Ident`.
    pub fn parent_ident_name(&self, parent: Option<ExprId>) -> Option<&str> {
        match self.get_expr(parent?) {
            Expr::Ident(n) => Some(n.as_str()),
            _ => None,
        }
    }
}
