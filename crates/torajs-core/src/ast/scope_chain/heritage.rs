//! Which class heritages name a binding a scope in between gives a
//! top-level class's name.
//!
//! `class base {}; function t(base) { class D extends base {} }` —
//! the heritage reads the parameter, not the class. A class pass that
//! keys a parent by name alone hands `D` the top-level `base`, which
//! is a silent wrong program whenever `t` is passed anything else.
//! The innermost binding of the name decides: a class declared in a
//! frame around `D` is still a class parent, any other binding is a
//! value.

use super::{Walk, names_of, walk};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// The heritage `Ident`s whose innermost binding is a frame's
/// non-class binding.
pub(in crate::ast) fn rebound_heritages(ast: &Ast, roots: &[&[Stmt]]) -> HashSet<ExprId> {
    walk(ast, roots)
        .heritages
        .into_iter()
        .filter_map(|(e, rebound)| rebound.then_some(e))
        .collect()
}

impl Walk<'_> {
    /// Bind `list`'s names in the current frame, marking the class
    /// declarations among them.
    pub(super) fn bind_list(&mut self, list: &[Stmt]) {
        names_of(list, true, &mut self.chain, Some(&mut self.classes));
    }

    /// Leave a frame: drop its names and its class marks.
    pub(super) fn truncate(&mut self, depth: usize) {
        self.chain.truncate(depth);
        while self.classes.last().is_some_and(|&i| i >= depth) {
            self.classes.pop();
        }
    }

    pub(super) fn note_heritage(&mut self, p: ExprId) {
        let Expr::Ident(n) = self.ast.get_expr(p) else {
            return;
        };
        let Some(i) = self.chain.iter().rposition(|c| c == n) else {
            return;
        };
        let rebound = self.classes.binary_search(&i).is_err();
        *self.heritages.entry(p).or_default() |= rebound;
    }
}
