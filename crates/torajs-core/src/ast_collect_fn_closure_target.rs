//! The forwarder-minting half of the call walk: sites that need a
//! top-FnDecl's `__forward_<f>` to exist while the site itself keeps
//! the raw function value, and the marking of a site whose value
//! becomes that forwarder's cell.

use crate::ast::{Expr, ExprId, Param};
use crate::ast_collect_fn_closure::FnToClosureCollector;
use std::collections::HashSet;

/// The params the closure-param tag pass gave the closure repr: each
/// already answers a cell, so [`FnToClosureCollector::try_mark`] counts
/// it among a choice's cell operands.
pub(crate) fn cls_params(params: &[Param]) -> HashSet<String> {
    let cls = |p: &&Param| {
        p.type_ann
            .as_deref()
            .is_some_and(|a| a.starts_with("__cls("))
    };
    params.iter().filter(cls).map(|p| p.name.clone()).collect()
}

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

    /// True when `eid` itself is the wrapped name. A choice (`c ? f1 :
    /// f2`, `f1 || f2`, `(0, f)`) whose operands are all named fns,
    /// function literals or closure-repr params wraps each named fn (a
    /// bare pointer of another origin among them would answer two
    /// representations); it answers false, since its condition still
    /// needs the caller's walk.
    pub(crate) fn try_mark(&mut self, eid: ExprId) -> bool {
        if self.mark_ident(eid) {
            return true;
        }
        let leaves = crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, eid);
        let all_cells = leaves.iter().all(|&leaf| {
            leaf != eid
                && (self.fn_name(leaf).is_some()
                    || match self.ast.get_expr(leaf) {
                        Expr::Closure { .. } | Expr::ArrowFn { .. } => true,
                        Expr::Ident(p) => {
                            self.cls_param_frames.last().is_some_and(|f| f.contains(p))
                        }
                        _ => false,
                    })
        });
        if all_cells {
            for leaf in leaves {
                self.mark_ident(leaf);
            }
        }
        false
    }

    fn fn_name(&self, eid: ExprId) -> Option<String> {
        match self.ast.get_expr(eid) {
            Expr::Ident(name)
                if self.fn_sigs.contains_key(name)
                    && !self.shadowed.iter().any(|s| s.contains(name)) =>
            {
                Some(name.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn mark_ident(&mut self, eid: ExprId) -> bool {
        let Some(name) = self.fn_name(eid) else {
            return false;
        };
        self.targets.insert(name.clone());
        self.rewrites.push((eid, name));
        true
    }
}
