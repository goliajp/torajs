//! Where the object casts (`super::obj`) read the annotation of the
//! slot a write stores into. A slot whose type the compiler inferred
//! has no annotation, and is not cast.

use crate::ast::{Expr, ExprId, Stmt};
use crate::check_type_ann::strip_nullable;
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// The annotation of the binding `name`: a module global's from its
    /// top-level declaration when `global`; otherwise a `let` of this
    /// function's from its declaration, a parameter's, or, for a binding
    /// this function captures, its declaration's elsewhere (by name).
    pub(super) fn binding_ann(&self, name: &str, global: bool) -> Option<String> {
        if global {
            return self.ast.stmts.iter().find_map(|s| let_ann(s, name));
        }
        if let Some(own) = self.let_anns.get(name) {
            return own.clone();
        }
        if let Some(p) = self.param(name) {
            return p;
        }
        let mut found = None;
        crate::ast::walk_stmts(&self.ast.stmts, &mut |s| {
            found = found.take().or_else(|| let_ann(s, name));
        });
        found
    }

    /// The annotation of the parameter `name` of the function being
    /// lowered: `None` when it has no such parameter, `Some(None)` for
    /// one without an annotation.
    fn param(&self, name: &str) -> Option<Option<String>> {
        self.ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name: f, params, ..
            } if *f == self.f.name => (params.iter())
                .find(|p| p.name == name)
                .map(|p| p.type_ann.clone()),
            _ => None,
        })
    }

    /// The annotation of the field `field` of the class instance `obj`
    /// reads — a class field typed by an object layout is always one
    /// the program wrote (an unannotated one is `any` unless its
    /// initializer is a scalar literal). `None` for any other receiver.
    pub(super) fn field_ann(&self, obj: ExprId, field: &str) -> Option<String> {
        crate::ast_throw_info::class_field_ann(self.ast, self.expr_types, obj, field)
    }

    /// The element annotation of the array the call taking `arg`
    /// appends to: a binding's or a class field's, peeled of its `[]` /
    /// `Array<>`.
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
        let ann = match self.ast.get_expr(recv) {
            Expr::Ident(n) => self.binding_ann(n, !self.locals.contains_key(n)),
            Expr::Member { obj, name } => self.field_ann(*obj, name),
            _ => None,
        }?;
        let ann = strip_nullable(&ann).trim();
        let elem = match ann.strip_suffix("[]") {
            Some(e) => e,
            None => ann.strip_prefix("Array<")?.strip_suffix('>')?,
        };
        Some(elem.trim().to_string())
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
