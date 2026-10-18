//! Where the object casts (`super::obj`) read the annotation of the
//! slot a write stores into. A slot whose type the compiler inferred
//! has no annotation, and is not cast.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::ValueId;
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// The annotation of the binding `name`: a module global's from its
    /// top-level declaration when `global`; otherwise that of the
    /// binding `name` denotes here, a `let` or parameter of this
    /// function, or for a binding a closure captures, the one it had
    /// where the closure was made. The same name declared in some other
    /// function or in a closed block is another binding, and is never
    /// read for this one.
    pub(crate) fn binding_ann(&self, name: &str, global: bool) -> Option<String> {
        if global {
            return self.ast.stmts.iter().find_map(|s| let_ann(s, name));
        }
        self.let_anns.get(&self.locals.get(name)?.slot).cloned()
    }

    /// Record the annotation of the `let` of `name` just lowered, when
    /// it bound a slot of its own (`outer` is the slot the name denoted
    /// before); a lane that kept the binding out of `locals` has none.
    pub(crate) fn note_binding_ann(
        &mut self,
        name: &str,
        outer: Option<ValueId>,
        ann: Option<&String>,
    ) {
        let (Some(ann), Some(l)) = (ann, self.locals.get(name)) else {
            return;
        };
        if outer != Some(l.slot) {
            self.let_anns.insert(l.slot, ann.clone());
        }
    }

    /// The annotation of the field `field` of the class instance `obj`
    /// reads — a class field typed by an object layout is always one
    /// the program wrote (an unannotated one is `any` unless its
    /// initializer is a scalar literal). `None` for any other receiver.
    pub(super) fn field_ann(&self, obj: ExprId, field: &str) -> Option<String> {
        crate::ast_throw_info::class_field_ann(self.ast, self.expr_types, obj, field)
    }

    /// The annotation the program wrote for the field `field` of the
    /// receiver `obj`: a class instance's declaration, or the field the
    /// receiver's own written annotation (`ast_throw_info::written_ann`)
    /// declares, so a binding typed by a declared or literal object type
    /// answers as a class instance does. `None` where a step is the
    /// compiler's inference.
    pub(crate) fn member_field_ann(&self, obj: ExprId, field: &str) -> Option<String> {
        if let Some(ann) = self.field_ann(obj, field) {
            return Some(ann);
        }
        let binding = |n: &str| self.binding_ann(n, !self.locals.contains_key(n));
        let class_of = |e| crate::ast_throw_info::class_of(self.expr_types, e);
        let owner = crate::ast_throw_info::written_ann(self.ast, obj, &binding, &class_of)?;
        crate::ssa_lower_object_lit::declared::field_ann(self, &owner, &field.into())
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
        crate::ast_throw_info::elem_ann(self.ast, &ann)
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
