//! The guard-dominated bounds proof, riding along the constraint
//! walk. `walk.rs` answers what each node contributes to the width
//! fixpoint; this answers which `(i, xs)` pairs stand while it does,
//! and therefore which index reads come out proven in-bounds.
//!
//! It lives on this walk rather than beside it because the element
//! width and the proof have to be one judgment — see the
//! `index_read_proven` field on `WidthTable`, and
//! [`crate::ssa_lower_bounds_proven`] for what a pair is and what
//! taints one.

use super::Analysis;
use crate::ast::{Expr, ExprId, Stmt};

impl Analysis<'_> {
    /// Walk something with the pair `cond` proves — if it proves one
    /// — standing, then drop it again.
    pub(super) fn bounds_guarded(&mut self, cond: Option<ExprId>, inner: impl FnOnce(&mut Self)) {
        let ast = self.ast;
        let pair = cond.and_then(|c| crate::ssa_lower_bounds_proven::guard_pair(ast, c));
        if let Some(p) = pair.clone() {
            self.bounds_stack.push(p);
        }
        inner(self);
        if let Some(p) = pair {
            self.bounds_stack.retain(|q| *q != p);
        }
    }

    /// Drop every pair this statement taints, before it is walked —
    /// the eviction is positional within a sequence, so a read after
    /// the tainting statement is no longer admitted while one before
    /// it still is.
    pub(super) fn bounds_evict(&mut self, s: &Stmt) {
        if self.bounds_stack.is_empty() {
            return;
        }
        let ast = self.ast;
        self.bounds_stack
            .retain(|(i, xs)| !crate::ssa_lower_bounds_proven::stmt_taints(ast, s, i, xs));
    }

    /// Record `obj[index]` as proven when it is exactly the `xs[i]`
    /// of a standing pair.
    pub(super) fn bounds_record(&mut self, eid: ExprId, obj: ExprId, index: ExprId) {
        if self.bounds_stack.is_empty() {
            return;
        }
        let ast = self.ast;
        let Expr::Ident(xs) = ast.get_expr(obj) else {
            return;
        };
        let Expr::Ident(i) = ast.get_expr(index) else {
            return;
        };
        if self
            .bounds_stack
            .iter()
            .any(|(pi, pxs)| pi == i && pxs == xs)
        {
            self.proven_reads.insert(eid);
        }
    }
}
