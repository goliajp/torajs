//! `class x extends x {}` — the heritage clause reads the class's own
//! binding while it is still uninitialized (ES §15.7.14 class
//! definition evaluation creates the name binding BEFORE evaluating
//! the heritage), so evaluating the definition throws a
//! ReferenceError at runtime.
//!
//! tr's class machinery is compile-time (hoisted class_index, chain
//! walks, global registration) — a self-referential parent used to
//! spin every parent-chain walk forever (the test262
//! `name-binding/in-extends-expression` tr-timeout). The rewrite
//! happens HERE, at parse time, before any hoisting moves the decl
//! away from its evaluation point: the declaration becomes a plain
//! `throw __new_ReferenceError(...)` statement (the
//! `dflt_param_tdz` factory-call shape), which throws exactly where
//! the class definition would have been evaluated.

use super::Parser;
use crate::ast::{Expr, ExprId, Stmt};

/// The parent name when the class extends its own name, else None.
/// A non-Ident heritage cannot spell the bare self-name, so it
/// answers None here (its subexpressions referencing the name are
/// the runtime's problem, same as any other capture).
pub(super) fn self_extends(ast: &crate::ast::Ast, stmt: &Stmt) -> Option<String> {
    if let Stmt::ClassDecl { name, parent, .. } = stmt
        && ast.parent_ident_name(*parent) == Some(name.as_str())
    {
        return Some(name.clone());
    }
    None
}

/// Expression-position twin — a named class expression carries a
/// SYNTH binding name (`__ClassExpr_<id>`) with the user's name
/// parked in `class_expr_display_names`, so the self-reference
/// compares the heritage against the display name.
pub(super) fn expr_self_extends(ast: &crate::ast::Ast, stmt: &Stmt) -> Option<String> {
    if let Stmt::ClassDecl { name, parent, .. } = stmt
        && let Some(p) = ast.parent_ident_name(*parent)
        && (p == name || ast.class_expr_display_names.get(name).map(String::as_str) == Some(p))
    {
        return Some(p.to_string());
    }
    None
}

impl Parser<'_> {
    /// Statement-position class declaration: a self-extending class
    /// becomes `throw __new_ReferenceError("Cannot access '<name>'
    /// before initialization")` in place.
    pub(super) fn finish_class_decl_stmt(&mut self, stmt: Stmt) -> Stmt {
        match self_extends(&self.ast, &stmt) {
            Some(name) => Stmt::Throw(self.tdz_reference_error(&name)),
            None => stmt,
        }
    }

    /// Expression-position class (`const C = class x extends x {}`,
    /// same TDZ: the inner name binds in the class environment before
    /// the heritage evaluates): an immediately-invoked throwing arrow,
    /// the `dflt_param_tdz::rewrite_to_throw_iife` shape.
    pub(super) fn class_expr_self_tdz(&mut self, name: &str) -> ExprId {
        let err = self.tdz_reference_error(name);
        let arrow = self.ast.add_expr(Expr::ArrowFn {
            params: vec![],
            return_type: Some("any".to_string()),
            body: vec![Stmt::Throw(err)],
        });
        self.ast.add_expr(Expr::Call {
            callee: arrow,
            args: vec![],
        })
    }

    /// `__new_ReferenceError("Cannot access '<name>' before
    /// initialization")` — the injected native-error factory call
    /// (resolves at check time; `inject_builtin_classes` provides it
    /// for every program).
    fn tdz_reference_error(&mut self, name: &str) -> ExprId {
        let msg = self.ast.add_expr(Expr::String(format!(
            "Cannot access '{name}' before initialization"
        )));
        let factory = self
            .ast
            .add_expr(Expr::Ident("__new_ReferenceError".to_string()));
        self.ast.add_expr(Expr::Call {
            callee: factory,
            args: vec![msg],
        })
    }
}
