//! The reference census behind a unit class's admission
//! (`class_unit`): where a class's name occurs, and what else a
//! subtree holds that the whitelist refuses. Split out of `class_unit`
//! ahead of K5, which grows the admission side. Verbatim move.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, StaticInit, Stmt};

pub(super) fn names_class(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Ident(n) if n == name)
        || matches!(e, Expr::New { class_name, .. } if class_name == name)
}

pub(super) fn returns_value(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::FnDecl { .. } => false,
        other => stmt_children_ref(other).into_iter().any(returns_value),
    }
}

/// Occurrences of the class name (and of `arguments`) in a subtree,
/// every nested function and class body included.
#[derive(Default)]
pub(super) struct Occurrences {
    pub(super) name: usize,
    pub(super) arguments: bool,
    /// A `function` expression (not an arrow): its own `this`.
    pub(super) fn_exprs: bool,
    /// Leave class bodies out. A class body is hoisted out of the
    /// container, so a name it holds stops reading the container's
    /// binding: for the question "does every reference stay in the
    /// container" such an occurrence is NOT a local one.
    pub(super) skip_class_bodies: bool,
    /// A class declaration anywhere in the subtree (a class expression
    /// spliced beside its use site included), whatever
    /// `skip_class_bodies` says about its body.
    pub(super) holds_class: bool,
}

impl Occurrences {
    pub(super) fn list(&mut self, ast: &Ast, stmts: &[Stmt], name: &str) {
        for s in stmts {
            self.stmt(ast, s, name);
        }
    }

    pub(super) fn stmt(&mut self, ast: &Ast, s: &Stmt, name: &str) {
        if matches!(s, Stmt::ClassDecl { .. }) {
            self.holds_class = true;
        }
        match s {
            Stmt::FnDecl { body, .. } => self.list(ast, body, name),
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } if !self.skip_class_bodies => {
                if let Some(c) = ctor {
                    self.list(ast, &c.body, name);
                }
                for m in methods.iter().chain(static_methods) {
                    self.list(ast, &m.body, name);
                }
                for i in static_init {
                    match i {
                        StaticInit::Block(b) => self.list(ast, b, name),
                        StaticInit::Field(f) => self.expr(ast, f.init, name),
                    }
                }
            }
            _ => {}
        }
        // An arrow's body is reached through its own node.
        for e in stmt_exprs(s) {
            self.expr(ast, e, name);
        }
        for c in stmt_children_ref(s) {
            self.stmt(ast, c, name);
        }
    }

    pub(super) fn expr(&mut self, ast: &Ast, eid: ExprId, name: &str) {
        let e = ast.get_expr(eid);
        if names_class(e, name) {
            self.name += 1;
        }
        if matches!(e, Expr::Ident(n) if n == "arguments") {
            self.arguments = true;
        }
        if let Expr::ArrowFn { body, .. } = e {
            self.fn_exprs |= ast.fn_expr_exprs.contains(&eid);
            self.list(ast, body, name);
            return;
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c, name);
        }
    }
}
