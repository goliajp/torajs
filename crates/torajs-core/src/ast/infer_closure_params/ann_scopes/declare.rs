//! The bindings a statement list declares in the scope it opens, the
//! `var`s it hoists into its function's, and what the unannotated ones
//! take from their initializers.

use super::Builder;
use crate::ast::infer_closure_lets::let_init_ann;
use crate::ast::{Expr, ExprId, Stmt};

impl<'a> Builder<'a> {
    /// The lexical declarations a statement list makes in its own
    /// scope; a `Multi` is spliced into the list it sits in.
    pub(super) fn declare(&mut self, scope: u32, list: &'a [Stmt]) {
        for s in list {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    is_var: false,
                    ..
                } => self.binding(scope, name, type_ann, *init),
                Stmt::Multi(inner) => self.declare(scope, inner),
                _ => {}
            }
        }
    }

    /// `var`s belong to the function (or the module) wherever they sit.
    pub(super) fn hoist_vars(&mut self, scope: u32, list: &'a [Stmt]) {
        for s in list {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    is_var: true,
                    ..
                } => self.binding(scope, name, type_ann, *init),
                Stmt::FnDecl { .. } => {}
                _ => crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |l| {
                    self.hoist_vars(scope, l)
                }),
            }
        }
    }

    fn binding(&mut self, scope: u32, name: &'a str, ann: &Option<String>, init: ExprId) {
        if ann.is_none() {
            self.unannotated.push((scope, name, init));
        }
        self.bind(scope, name, ann.clone());
    }

    /// An unannotated binding that no annotated declaration of the same
    /// scope answers for takes what its initializer gives it. A callee
    /// that an enclosing function rebinds is not the module's function
    /// of that name, so its declared return says nothing.
    pub(super) fn infer_unannotated(&mut self) {
        for (scope, name, init) in std::mem::take(&mut self.unannotated) {
            if self.scopes[scope as usize].names[name].is_some() {
                continue;
            }
            let mut leaf = init;
            while let Expr::Array(els) = self.ast.get_expr(leaf)
                && let Some(&first) = els.first()
            {
                leaf = first;
            }
            if let Expr::Call { callee, .. } = self.ast.get_expr(leaf)
                && let Expr::Ident(f) = self.ast.get_expr(*callee)
                && self.rebound_below_module(scope, f)
            {
                continue;
            }
            let ann = let_init_ann(self.ast, init, self.declared_rets);
            self.bind(scope, name, ann);
        }
    }

    fn rebound_below_module(&self, scope: u32, name: &str) -> bool {
        let mut s = scope;
        loop {
            let sc = &self.scopes[s as usize];
            let Some(parent) = sc.parent else {
                return false;
            };
            if sc.names.contains_key(name) {
                return true;
            }
            s = parent;
        }
    }
}
