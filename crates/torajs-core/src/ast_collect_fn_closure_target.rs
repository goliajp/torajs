//! The forwarder-minting half of the call walk: sites that need a
//! top-FnDecl's `__forward_<f>` to exist while the site itself keeps
//! the raw function value.

use crate::ast::{Expr, ExprId};
use crate::ast_collect_fn_closure::FnToClosureCollector;

impl FnToClosureCollector<'_> {
    /// Rotation 652 — a top-FnDecl handler of a `.then` / `.catch` /
    /// `.finally` call gets a FORWARDER but keeps its raw FnSig at
    /// the site: the typed chain lane dispatches the bare address
    /// (`promise-then-100k` / `promise-chain-1m` are exactly
    /// `.then(add1)`), and only the own-`then` split's patched block
    /// (`ssa_lower_promise_chain_own_then`) needs a closure cell —
    /// which it builds from `__forward_<f>` by name. Target only, no
    /// rewrite — and minted even when no other site rewrites (r653).
    pub(crate) fn mark_promise_chain_handler_targets(&mut self, callee: &ExprId, args: &[ExprId]) {
        if let Expr::Member { name, .. } = self.ast.get_expr(*callee)
            && matches!(name.as_str(), "then" | "catch" | "finally")
        {
            self.target_fn_args(args);
        }
    }

    /// Mint the forwarder of every top-FnDecl ident in `args`, leaving
    /// the sites as written. An operator that hands an operand
    /// straight through (`c ? f1 : f2`, `f1 || f2`) is looked through.
    pub(crate) fn target_fn_args(&mut self, args: &[ExprId]) {
        for &arg in args {
            self.target_fn_value(arg);
        }
    }

    fn target_fn_value(&mut self, e: ExprId) {
        match self.ast.get_expr(e) {
            Expr::Ident(f)
                if self.fn_sigs.contains_key(f) && !self.shadowed.iter().any(|s| s.contains(f)) =>
            {
                self.targets.insert(f.clone());
            }
            Expr::Ternary {
                then_branch: a,
                else_branch: b,
                ..
            }
            | Expr::Nullish { lhs: a, rhs: b }
            | Expr::BinOp {
                op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
                left: a,
                right: b,
            } => {
                let (a, b) = (*a, *b);
                self.target_fn_value(a);
                self.target_fn_value(b);
            }
            Expr::Sequence { right: v, .. } | Expr::As { expr: v, .. } => {
                let v = *v;
                self.target_fn_value(v);
            }
            _ => {}
        }
    }
}
