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

use super::{Walk, list_names, list_names_but_classes, walk};
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
    /// Mark the `chain` entries from `depth` on that `list` put there
    /// for a class declaration.
    pub(super) fn mark_classes(&mut self, list: &[Stmt], depth: usize) {
        let mut all = Vec::new();
        let mut plain = Vec::new();
        list_names(list, &mut all);
        list_names_but_classes(list, &mut plain);
        all.retain(|n| !plain.contains(n));
        for i in depth..self.chain.len() {
            if all.contains(&self.chain[i]) && !self.classes.contains(&i) {
                self.classes.push(i);
            }
        }
    }

    /// Leave a frame: drop its names and its class marks.
    pub(super) fn truncate(&mut self, depth: usize) {
        self.chain.truncate(depth);
        self.classes.retain(|&i| i < depth);
    }

    pub(super) fn note_heritage(&mut self, p: ExprId) {
        let Expr::Ident(n) = self.ast.get_expr(p) else {
            return;
        };
        let Some(i) = self.chain.iter().rposition(|c| c == n) else {
            return;
        };
        let rebound = !self.classes.contains(&i);
        *self.heritages.entry(p).or_default() |= rebound;
    }
}
