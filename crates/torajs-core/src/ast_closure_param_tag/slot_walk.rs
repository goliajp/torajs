//! The scope walk behind
//! [`crate::ast_closure_param_tag::slot_bind::slot_bound_reads`]: block,
//! function and capture scopes, each frame naming its bindings and
//! whether each is a slot-read binding.

use crate::ast::desugar_async_sm_walk::visit_expr_children;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_closure_param_tag::slot_bind::holds_slot_read;
use std::collections::{HashMap, HashSet};

pub(crate) struct Walk<'a> {
    pub(crate) ast: &'a Ast,
    /// innermost last: name → whether it is a slot-read binding
    pub(crate) frames: Vec<HashMap<String, bool>>,
    pub(crate) reads: HashSet<ExprId>,
    pub(crate) inherited: HashMap<String, HashSet<String>>,
    /// the declarations lifted out of a closure, with their params:
    /// each body sees its defining scope only through its captures,
    /// not through the top level it is placed in
    pub(crate) lifted: &'a HashMap<&'a str, &'a [Param]>,
    /// the nested declarations lifted to the top level, with their
    /// params: a default each keeps is pasted into its calls, which sit
    /// in the function it was declared in
    pub(crate) padded: &'a HashMap<&'a str, &'a [Param]>,
}

impl Walk<'_> {
    fn resolves(&self, n: &str) -> bool {
        let found = self.frames.iter().rev().find_map(|f| f.get(n));
        found.copied().unwrap_or(false)
    }

    fn visible(&self) -> HashSet<String> {
        let mut names: HashSet<&str> = HashSet::new();
        for f in &self.frames {
            names.extend(f.keys().map(String::as_str));
        }
        names
            .into_iter()
            .filter(|n| self.resolves(n))
            .map(str::to_string)
            .collect()
    }

    pub(crate) fn block(&mut self, stmts: &[Stmt]) {
        let refs: Vec<&Stmt> = stmts.iter().collect();
        self.scope(&refs, |_| {});
    }

    /// A block scope over `stmts`, whose lexical bindings are in scope
    /// throughout; `rest` runs inside it after them.
    fn scope(&mut self, stmts: &[&Stmt], rest: impl FnOnce(&mut Self)) {
        let mut frame: HashMap<String, bool> = HashMap::new();
        let mut decls: Vec<&Stmt> = stmts.to_vec();
        while let Some(s) = decls.pop() {
            match s {
                Stmt::Multi(inner) => decls.extend(inner),
                Stmt::ExportDecl { inner: Some(i), .. } => decls.push(i),
                Stmt::LetDecl {
                    name,
                    init,
                    type_ann,
                    is_var: false,
                    ..
                } => {
                    let q = holds_slot_read(self.ast, type_ann.as_deref(), *init);
                    *frame.entry(name.clone()).or_default() |= q;
                }
                Stmt::UsingDecl { name, .. } | Stmt::FnDecl { name, .. } => {
                    frame.entry(name.clone()).or_default();
                }
                _ => {}
            }
        }
        self.frames.push(frame);
        for s in stmts {
            self.stmt(s);
        }
        rest(self);
        self.frames.pop();
    }

    /// A scope binding only `names`, none of them slot reads.
    fn scoped(&mut self, names: &[&String], f: impl FnOnce(&mut Self)) {
        let frame = names.iter().map(|n| (n.to_string(), false)).collect();
        self.frames.push(frame);
        f(self);
        self.frames.pop();
    }

    /// The param defaults, read in the params' scope, which has none
    /// of the body's bindings.
    pub(crate) fn defaults(&mut self, params: &[Param]) {
        params
            .iter()
            .filter_map(|p| p.default)
            .for_each(|d| self.expr(d));
    }

    fn sub(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(b) => self.block(b),
            _ => self.block(std::slice::from_ref(s)),
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) | Stmt::Return(Some(e)) => {
                self.expr(*e)
            }
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => self.expr(*init),
            Stmt::YieldInto { value, .. } => self.expr(*value),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.sub(then_branch);
                if let Some(b) = else_branch {
                    self.sub(b);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.expr(*cond);
                self.sub(body);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                let bodies = cases.iter().flat_map(|c| &c.body);
                let all: Vec<&Stmt> = bodies.chain(default.iter().flatten()).collect();
                self.scope(&all, |w| cases.iter().for_each(|c| w.expr(c.value)));
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let head: Vec<&Stmt> = init.iter().map(|i| &**i).collect();
                self.scope(&head, |w| {
                    cond.iter().chain(step.iter()).for_each(|e| w.expr(*e));
                    w.sub(body);
                });
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
                ..
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.scoped(&[var_name], |w| w.sub(body));
            }
            Stmt::ForOf {
                var_name,
                src_ident,
                i_ident,
                elem_expr,
                body,
                forin_obj,
                ..
            } => {
                if let Some(o) = forin_obj {
                    self.expr(*o);
                }
                self.scoped(&[var_name, src_ident, i_ident], |w| {
                    w.expr(*elem_expr);
                    w.sub(body);
                });
            }
            Stmt::Labeled { body, .. } => self.sub(body),
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(body);
                let cp: Vec<&String> = catch_param.iter().collect();
                self.scoped(&cp, |w| w.block(catch_body));
                if let Some(f) = finally_body {
                    self.block(f);
                }
            }
            Stmt::Block(b) => self.block(b),
            Stmt::Multi(b) => b.iter().for_each(|s| self.stmt(s)),
            Stmt::FnDecl { name, .. } if !self.lifted.contains_key(name.as_str()) => {
                let seen = self.visible();
                self.inherited.entry(name.clone()).or_default().extend(seen);
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.stmt(i);
                }
                if let Some(e) = default_expr {
                    self.expr(*e);
                }
            }
            Stmt::FnDecl { .. }
            | Stmt::Return(None)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::TypeDecl { .. }
            | Stmt::ClassDecl { .. }
            | Stmt::ImportDecl { .. } => {}
        }
    }

    fn expr(&mut self, e: ExprId) {
        match self.ast.get_expr(e) {
            Expr::Ident(n) if self.resolves(n) => {
                self.reads.insert(e);
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(f) = self.ast.get_expr(*callee)
                    && let Some(params) = self.padded.get(f.as_str()).copied()
                {
                    self.defaults(params);
                }
            }
            Expr::Closure { fn_name, captures } => {
                let caught = captures.iter().filter(|c| self.resolves(c)).cloned();
                let caught: Vec<String> = caught.collect();
                self.inherited
                    .entry(fn_name.clone())
                    .or_default()
                    .extend(caught);
                // a default the closure kept reads names where the closure
                // is written; they need not be among its captures
                let params = self.lifted.get(fn_name.as_str()).copied().unwrap_or(&[]);
                let names: Vec<&String> = params.iter().map(|p| &p.name).collect();
                self.scoped(&names, |w| w.defaults(params));
            }
            _ => {}
        }
        visit_expr_children(self.ast, e, &mut |c| self.expr(c));
    }
}
