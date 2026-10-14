//! The forwarder-minting half of the call walk: sites that need a
//! top-FnDecl's `__forward_<f>` to exist while the site itself keeps
//! the raw function value, and the marking of a site whose value
//! becomes that forwarder's cell.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_collect_fn_closure::FnToClosureCollector;
use std::collections::{HashMap, HashSet};

pub(crate) mod raw_ret;

type FnSig = (Vec<Param>, Option<String>, crate::lexer::Span);

/// The params the closure-param tag pass gave the closure repr: each
/// already answers a cell, so [`FnToClosureCollector::try_mark`] counts
/// it among a choice's cell operands. An optional one (`o?: (s: string)
/// => number`) is spelled `__nullable(__cls(...))` and holds the same
/// cell or nothing.
pub(crate) fn cls_params(params: &[Param]) -> HashSet<String> {
    let cls = |p: &&Param| p.type_ann.as_deref().is_some_and(raw_ret::is_cls_ann);
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
        let nullish = |leaf| match self.ast.get_expr(leaf) {
            Expr::Null => true,
            Expr::Ident(p) => p == "undefined",
            _ => false,
        };
        let all_cells = leaves.iter().all(|&leaf| {
            leaf != eid && (self.fn_name(leaf).is_some() || self.is_cell(leaf) || nullish(leaf))
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
        params: &[Param],
        return_type: &Option<String>,
        body: &[Stmt],
    ) {
        if !crate::ast_closure_param_tag::is_fnsig_ann(return_type) {
            return;
        }
        let rets = fn_returns(body);
        let mut named: Vec<String> = rets.iter().filter_map(|&r| self.fn_name(r)).collect();
        let direct = named.len();
        // a const binding of a named function returned in its place
        // (`raw_ret`) needs the same fn object
        let lets = raw_ret::const_lets(body);
        let is_fn = |n: &str| self.fn_sigs.contains_key(n);
        for &r in &rets {
            if self.fn_name(r).is_none()
                && let Some(raw_ret::RawRet::Named(o)) =
                    raw_ret::raw_ret(self.ast, params, &lets, r, &is_fn)
            {
                named.push(o);
            }
        }
        // so does a named function among the arms of a returned choice,
        // which the lane splits into one return per arm
        // (`check_monomorph::any_ret`)
        for &r in &rets {
            let leaves = crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, r);
            if leaves != [r] {
                named.extend(leaves.into_iter().filter_map(|l| self.fn_name(l)));
            }
        }
        if !named.is_empty() && (direct < rets.len() || self.mixed_slot_rows.contains(name)) {
            self.targets.extend(named);
        }
    }

    /// A function literal, or a param the closure-param tag pass gave
    /// the closure repr: a value that already is a fn cell.
    fn is_cell(&self, leaf: ExprId) -> bool {
        match self.ast.get_expr(leaf) {
            Expr::Closure { .. } | Expr::ArrowFn { .. } => true,
            Expr::Ident(p) => self.cls_param_frames.last().is_some_and(|f| f.contains(p)),
            _ => false,
        }
    }

    /// A choice mixing named fns with values that are not fn cells
    /// (`(flip && inc) || h`) joins in the any lane, where a named fn
    /// boxes as its canonical cell: the forwarder that cell runs is
    /// synthesized here, the site itself stays as written.
    ///
    /// Two shapes rewrite the named leaves to their cells instead. A
    /// generic function (explicit, or with an unannotated parameter)
    /// has no raw value at all, only the forwarder's instance at
    /// `any`. And next to a fn cell (`opt || decode` with `opt` a
    /// closure-repr param) the raw pointer would be a second
    /// representation of the same fn-typed value, called through the
    /// cell's entry.
    ///
    /// A choice among named fns alone joins in the any lane too when
    /// their signatures differ: the checker types it as their union,
    /// whose value is the chosen operand's fn object. A face the
    /// declarations do not spell out (an inferred return) counts as
    /// differing: a shell no site boxes is never called, a missing one
    /// refuses the box.
    pub(crate) fn target_mixed_choice(&mut self, eid: ExprId) {
        let leaves = crate::ast_closure_param_tag_member::passthrough_leaves(self.ast, eid);
        let named: Vec<(ExprId, String)> = (leaves.iter())
            .filter_map(|&l| Some((l, self.fn_name(l)?)))
            .collect();
        if named.is_empty() {
            return;
        }
        let others: Vec<ExprId> = (leaves.iter())
            .filter(|l| !named.iter().any(|(n, _)| n == *l))
            .copied()
            .collect();
        let rewrite = named.iter().any(|&(l, _)| self.is_no_original_fn_ident(l))
            || (!others.is_empty() && others.iter().all(|&l| self.is_cell(l)));
        if rewrite {
            for (leaf, _) in named {
                self.mark_ident(leaf);
            }
        } else if !others.is_empty() || !self.same_declared_face(&named) {
            self.targets.extend(named.into_iter().map(|(_, n)| n));
        }
    }

    /// Do the declarations of `named` spell one signature: the same
    /// parameter list (annotation, rest, default) and the same
    /// explicit return annotation, none of them generic?
    fn same_declared_face(&self, named: &[(ExprId, String)]) -> bool {
        let face = |n: &str| {
            let (params, ret, _) = &self.fn_sigs[n];
            let ps: Vec<_> = (params.iter())
                .map(|p| (p.type_ann.as_deref(), p.is_rest, p.default.is_some()))
                .collect();
            (ps, ret.as_deref())
        };
        let first = face(&named[0].1);
        first.1.is_some()
            && named
                .iter()
                .all(|(_, n)| !self.fn_type_params.contains_key(n) && face(n) == first)
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

    /// `eid` is a bare name that is not a generator or async function:
    /// those carry their own value shapes, so only a plain declaration
    /// is wrapped into its canonical cell on that account.
    pub(crate) fn names_plain_fn(&self, eid: ExprId) -> bool {
        matches!(self.ast.get_expr(eid), Expr::Ident(n)
            if !self.ast.generator_factory_classes.contains_key(n)
                && !self.ast.async_fns.contains(n)
                && !self.ast.async_generator_fns.contains(n))
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

/// The method rows declared to return a fn type in a vtable slot whose
/// rows, between them, return both named functions and other values
/// (`get() { return dbl; }` overridden by `get() { return v; }`). Once
/// one row's return goes through the closure lane every row's does, so
/// the named functions there need their fn objects. A row whose return
/// the closure-param pass already gave the closure repr is in the lane
/// from the start, and counts among the rows.
pub(crate) fn mixed_slot_rows(ast: &Ast, fn_sigs: &HashMap<String, FnSig>) -> HashSet<String> {
    type Row<'a> = (&'a Option<String>, &'a [Param], &'a [Stmt]);
    let bodies: HashMap<&str, Row> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                ..
            } => Some((
                name.as_str(),
                (return_type, params.as_slice(), body.as_slice()),
            )),
            _ => None,
        })
        .collect();
    let is_fn = |n: &str| fn_sigs.contains_key(n);
    let mut out = HashSet::new();
    for (_, by_root) in crate::ast::vtable_slot::slot_groups_by_name(ast) {
        for rows in by_root.values() {
            let rows: Vec<&String> = rows
                .iter()
                .filter(|r| {
                    bodies.get(r.as_str()).is_some_and(|(ann, ..)| {
                        crate::ast_closure_param_tag::is_fnsig_ann(ann)
                            || ann.as_deref().is_some_and(raw_ret::is_cls_ann)
                    })
                })
                .collect();
            let (mut named, mut raw, mut n) = (0, 0, 0);
            for r in &rows {
                let (_, params, body) = bodies[r.as_str()];
                let (rr, rn) = raw_ret::decl_raw_rets(ast, params, body, &is_fn);
                named += fn_returns(body)
                    .iter()
                    .filter(|&&e| matches!(ast.get_expr(e), Expr::Ident(n) if is_fn(n)))
                    .count();
                raw += rr.len();
                n += rn;
            }
            if (named > 0 && named < n) || (raw > 0 && raw < n) {
                out.extend(rows.into_iter().cloned());
            }
        }
    }
    out
}
