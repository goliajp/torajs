//! Where the object casts (`super::obj`) read the annotation of the
//! slot a write stores into. A slot whose type the compiler inferred
//! has no annotation, and is not cast.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// The annotation of the binding `name`: a module global's from its
    /// top-level declaration when `global`; otherwise the latest `let`
    /// or parameter of this function by that name, or for a binding a
    /// closure captures, the one it had where the closure was made. The
    /// same name declared in some other function is another binding,
    /// and is never read for this one.
    pub(crate) fn binding_ann(&self, name: &str, global: bool) -> Option<String> {
        if global {
            return self.ast.stmts.iter().find_map(|s| let_ann(s, name));
        }
        self.let_anns.get(name).cloned().flatten()
    }

    /// The annotation of the field `field` of the class instance `obj`
    /// reads — a class field typed by an object layout is always one
    /// the program wrote (an unannotated one is `any` unless its
    /// initializer is a scalar literal). `None` for any other receiver.
    pub(super) fn field_ann(&self, obj: ExprId, field: &str) -> Option<String> {
        crate::ast_throw_info::class_field_ann(self.ast, self.expr_types, obj, field)
    }

    /// The element annotation of the array the call taking `arg`
    /// appends to, as the checker traced it when it admitted the call
    /// (`ast_throw_info::written_ann`).
    pub(super) fn pushed_elem_ann(&self, arg: ExprId) -> Option<String> {
        let recv = self.ast.exprs.iter().find_map(|e| match e {
            Expr::Call { callee, args } if args.contains(&arg) => {
                match self.ast.get_expr(*callee) {
                    Expr::Member { obj, .. } => Some(*obj),
                    _ => None,
                }
            }
            _ => None,
        })?;
        let binding = |n: &str| self.binding_ann(n, !self.locals.contains_key(n));
        let class_of = |e| crate::ast_throw_info::class_of(self.expr_types, e);
        let ann = crate::ast_throw_info::written_ann(self.ast, recv, &binding, &class_of)?;
        crate::ast_throw_info::elem_ann(&ann)
    }
}

/// The annotation `s` declares `name` with, when it is a `let` of it.
fn let_ann(s: &Stmt, name: &str) -> Option<String> {
    match s {
        Stmt::LetDecl {
            name: n,
            type_ann: Some(a),
            ..
        } if n == name => Some(a.clone()),
        _ => None,
    }
}
