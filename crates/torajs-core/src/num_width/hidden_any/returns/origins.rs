//! What the return walk of `hidden_any::returns` knows of the
//! declarations a name may read.

use crate::ast::{Expr, ExprId};
use crate::check::Type;
use crate::num_width::hidden_any::Graph;
use crate::num_width::hidden_any::{Node, Origin, Ref};
use crate::num_width::{Analysis, Scope, SlotKey};

impl Graph<'_> {
    /// The typed return of the type of `callee`, a binding or parameter
    /// outside the boxed-entry census (`Ast::face_mismatch_lets`,
    /// `Ast::argv_boxed_params`), whose calls answer what the stored
    /// function does.
    pub(super) fn typed_callee<'t>(
        &self,
        a: &'t Analysis<'_>,
        sc: &Scope,
        callee: ExprId,
    ) -> Option<&'t Type> {
        let (Expr::Ident(n), Some(Type::Function(_, r))) =
            (a.ast.get_expr(callee), a.expr_types.get(&callee))
        else {
            return None;
        };
        if **r == Type::Any || a.ast.face_mismatch_globals.contains(n) {
            return None;
        }
        let plain = match self.resolve(a, n, sc) {
            Ref::At(Node::Decl(i)) => match self.origin(i) {
                Origin::Fixed(v) | Origin::Open(v) => !a.ast.face_mismatch_lets.contains(&v),
                Origin::Param(_) | Origin::Unknown => false,
            },
            Ref::At(Node::Read(SlotKey::Param(f, p))) => {
                !a.ast.boxed_params_of(&f).is_some_and(|ps| ps.contains(&p))
            }
            _ => false,
        };
        plain.then_some(r)
    }

    /// What the declarations `n` may name say about their values; an
    /// open parameter spells `any`.
    pub(super) fn origins(&self, a: &Analysis<'_>, sc: &Scope, n: &str) -> Vec<Origin> {
        match self.resolve(a, n, sc) {
            Ref::At(Node::Decl(i)) => vec![self.origin(i)],
            Ref::At(Node::Read(SlotKey::Param(f, p))) => {
                let open = self.lookups(a).open_params.get(&(f, p)).copied();
                vec![Origin::Param(open.unwrap_or(true))]
            }
            Ref::Keyed(_) => match self.lookups(a).decls.get(n) {
                Some(os) => os.clone(),
                None => vec![Origin::Unknown],
            },
            Ref::At(_) => vec![Origin::Unknown],
        }
    }

    /// The values a binding `n` declared with `o` may hold, when the
    /// walk traces them.
    pub(super) fn values(&self, a: &Analysis<'_>, n: &str, o: Origin) -> Option<Vec<ExprId>> {
        match o {
            Origin::Fixed(v) => Some(vec![v]),
            Origin::Open(v) => {
                let mut vs = self.lookups(a).stores.get(n).cloned().unwrap_or_default();
                vs.push(v);
                Some(vs)
            }
            Origin::Param(_) | Origin::Unknown => None,
        }
    }

    /// [`super::Decl::origin`] of declaration `i`.
    pub(super) fn origin(&self, i: usize) -> Origin {
        match self.decls.get(i) {
            Some(d) => d.origin,
            None => (self.origins_in.get(i).copied()).unwrap_or(Origin::Unknown),
        }
    }
}
