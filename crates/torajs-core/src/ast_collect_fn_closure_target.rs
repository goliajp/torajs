//! The forwarder-minting half of the call walk: sites that need a
//! top-FnDecl's `__forward_<f>` to exist while the site itself keeps
//! the raw function value, and the marking of a site whose value
//! becomes that forwarder's cell.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_collect_fn_closure::FnToClosureCollector;
use std::collections::{HashMap, HashSet};

type FnSig = (Vec<Param>, Option<String>, crate::lexer::Span);

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

    /// A function declared to return a bare-pointer fn type, whose
    /// returns mix named functions with other values, returns through
    /// the closure lane once the checker types one of those values
    /// `any` (`check_monomorph::any_ret`); a named function returned
    /// there answers its fn object, whose forwarder is minted here. A
    /// method row mixes with the other rows of its vtable slot, which
    /// `any_ret` retags together ([`mixed_slot_rows`]).
    pub(crate) fn target_mixed_fn_returns(
        &mut self,
        name: &str,
        return_type: &Option<String>,
        body: &[Stmt],
    ) {
        if !crate::ast_closure_param_tag::is_fnsig_ann(return_type) {
            return;
        }
        let rets = fn_returns(body);
        let named: Vec<String> = rets.iter().filter_map(|&r| self.fn_name(r)).collect();
        if !named.is_empty() && (named.len() < rets.len() || self.mixed_slot_rows.contains(name)) {
            self.targets.extend(named);
        }
    }

    /// A choice mixing named fns with values that are not fn cells
    /// (`(flip && inc) || h`) joins in the any lane, where a named fn
    /// boxes as its canonical cell: the forwarder that cell runs is
    /// synthesized here, the site itself stays as written.
    pub(crate) fn target_mixed_choice(&mut self, eid: ExprId) {
        let leaves = crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, eid);
        let named: Vec<String> = leaves.iter().filter_map(|&l| self.fn_name(l)).collect();
        if !named.is_empty() && named.len() < leaves.len() {
            self.targets.extend(named);
        }
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

/// The values a body returns, not counting the returns of a function
/// nested in it.
pub(crate) fn fn_returns(body: &[Stmt]) -> Vec<ExprId> {
    let mut rets: Vec<ExprId> = Vec::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::Return(Some(e)) => rets.push(*e),
            Stmt::FnDecl { .. } => {}
            _ => crate::ast_closure_param_tag::push_child_stmts(s, &mut stack),
        }
    }
    rets
}

/// The method rows declared to return a bare-pointer fn type in a
/// vtable slot whose rows, between them, return both named functions
/// and other values (`get() { return dbl; }` overridden by `get() {
/// return v; }`). Once one row's return goes through the closure lane
/// every row's does, so the named functions there need their fn
/// objects.
pub(crate) fn mixed_slot_rows(ast: &Ast, fn_sigs: &HashMap<String, FnSig>) -> HashSet<String> {
    let bodies: HashMap<&str, (&Option<String>, &[Stmt])> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                return_type,
                body,
                ..
            } => Some((name.as_str(), (return_type, body.as_slice()))),
            _ => None,
        })
        .collect();
    let mut out = HashSet::new();
    for (_, by_root) in crate::ast::vtable_slot::slot_groups_by_name(ast) {
        for rows in by_root.values() {
            let rows: Vec<&String> = rows
                .iter()
                .filter(|r| {
                    bodies
                        .get(r.as_str())
                        .is_some_and(|(ann, _)| crate::ast_closure_param_tag::is_fnsig_ann(ann))
                })
                .collect();
            let rets: Vec<ExprId> = rows
                .iter()
                .flat_map(|r| fn_returns(bodies[r.as_str()].1))
                .collect();
            let named = rets
                .iter()
                .filter(|&&e| matches!(ast.get_expr(e), Expr::Ident(n) if fn_sigs.contains_key(n)))
                .count();
            if named > 0 && named < rets.len() {
                out.extend(rows.into_iter().cloned());
            }
        }
    }
    out
}
