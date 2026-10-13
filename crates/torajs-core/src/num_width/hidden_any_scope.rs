//! The block-scoped walk of one body for `hidden_any`. Each `let` /
//! `const` (and each for-of, catch and `using` binding) is its own
//! node, and a name resolves to the innermost declaration in scope, so
//! a write into one of two same-named bindings reaches only that one.
//! A lifted closure reads a captured name through the declaration its
//! construction site resolved the name to.

use std::collections::HashMap;

use super::captures::Body;
use super::hidden_any::{Decl, Graph, Node, Origin, candidate};
use super::{Analysis, Scope, SlotKey};
use crate::ast::desugar_async_sm_walk::visit_expr_children;
use crate::ast::{Expr, ExprId, Stmt};

impl Graph<'_> {
    /// The declarations, for-of bindings and assignments of one body.
    pub(super) fn walk_body(&mut self, a: &Analysis<'_>, b: &Body<'_>) {
        let sc = Scope {
            fn_name: &b.name,
            params: b.params.clone(),
            locals: b.locals.clone(),
        };
        let params = (b.params.iter())
            .map(|p| {
                (
                    p.clone(),
                    Node::Read(SlotKey::Param(b.name.clone(), p.clone())),
                )
            })
            .collect();
        self.frames = vec![params];
        let stmts: Vec<&Stmt> = b.stmts().collect();
        self.block(a, &sc, &stmts);
        self.frames.clear();
    }

    fn block(&mut self, a: &Analysis<'_>, sc: &Scope, stmts: &[&Stmt]) {
        self.frames.push(HashMap::new());
        self.predeclare(a, sc, stmts, false);
        for s in stmts {
            self.stmt(a, sc, s);
        }
        self.frames.pop();
    }

    /// The `let` / `const` / `using` bindings of one block exist from
    /// its start (§14.2.2), so a closure built before the declaration
    /// already names it; a `var` lives in the body's own frame.
    fn predeclare(&mut self, a: &Analysis<'_>, sc: &Scope, stmts: &[&Stmt], export: bool) {
        for s in stmts {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    is_var,
                    mutable,
                } => {
                    let at = if *is_var { 1 } else { self.frames.len() - 1 };
                    // a `var` declared twice is one binding
                    if !self.frames[at].contains_key(name) {
                        let cand = !export && candidate(a, type_ann.as_deref(), *init);
                        let origin = match type_ann {
                            None if !*mutable && !*is_var => Origin::Fixed(*init),
                            _ => Origin::Open(*init),
                        };
                        self.declare(sc, name, (cand.then_some(*init), origin), at);
                    }
                }
                Stmt::UsingDecl { name, .. } => {
                    self.declare(sc, name, (None, Origin::Unknown), self.frames.len() - 1);
                }
                Stmt::Multi(v) => self.predeclare(a, sc, &v.iter().collect::<Vec<_>>(), export),
                Stmt::ExportDecl { inner: Some(i), .. } => self.predeclare(a, sc, &[i], true),
                _ => {}
            }
        }
    }

    /// A fresh declaration of `n` in frame `at`; `held` is its candidate initializer and origin.
    fn declare(&mut self, sc: &Scope, n: &str, held: (Option<ExprId>, Origin), at: usize) -> Node {
        let (init, origin) = held;
        let key = if sc.fn_name.is_empty() {
            SlotKey::Global(n.to_string())
        } else {
            SlotKey::Local(sc.fn_name.to_string(), n.to_string())
        };
        let node = Node::Decl(self.decls.len());
        self.decls.push(Decl { key, init, origin });
        self.frames[at].insert(n.to_string(), node.clone());
        node
    }

    /// The declaration `n` names, stored `value`.
    fn store(&mut self, a: &Analysis<'_>, sc: &Scope, n: &str, value: ExprId) {
        self.exprs(a, sc, value);
        let node = match self.frames.iter().rev().find_map(|f| f.get(n)) {
            Some(node) => node.clone(),
            None => self.declare(sc, n, (None, Origin::Unknown), self.frames.len() - 1),
        };
        self.flow(a, &[node], value, sc);
    }

    /// A frame of its own for a loop or catch binding `n`, which stores
    /// `value` (and holds what it reads) when there is one, around `body`.
    fn bound(
        &mut self,
        a: &Analysis<'_>,
        sc: &Scope,
        n: &str,
        value: Option<ExprId>,
        body: &[&Stmt],
    ) {
        self.frames.push(HashMap::new());
        let origin = value.map_or(Origin::Unknown, Origin::Open);
        let node = self.declare(sc, n, (None, origin), self.frames.len() - 1);
        if let Some(v) = value {
            self.flow(a, &[node], v, sc);
        }
        self.block(a, sc, body);
        self.frames.pop();
    }

    fn stmt(&mut self, a: &Analysis<'_>, sc: &Scope, s: &Stmt) {
        match s {
            Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
                self.store(a, sc, name, *init)
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => self.exprs(a, sc, *e),
            Stmt::Return(Some(e)) => {
                self.exprs(a, sc, *e);
                self.returned(a, sc, *e);
                self.flow(a, &[Node::Ret(sc.fn_name.to_string())], *e, sc);
            }
            Stmt::YieldInto { value, .. } => self.exprs(a, sc, *value),
            Stmt::Labeled { body, .. } => self.stmt(a, sc, body),
            Stmt::Block(v) => self.block(a, sc, &v.iter().collect::<Vec<_>>()),
            Stmt::Multi(v) => {
                for st in v {
                    self.stmt(a, sc, st);
                }
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.stmt(a, sc, i);
                }
                if let Some(e) = default_expr {
                    self.exprs(a, sc, *e);
                }
            }
            Stmt::If { .. } | Stmt::Switch { .. } | Stmt::Try { .. } => self.branches(a, sc, s),
            Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::ForOfSplitIter { .. }
            | Stmt::ForOf { .. } => self.loops(a, sc, s),
            Stmt::Return(None)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::TypeDecl { .. }
            | Stmt::ImportDecl { .. }
            | Stmt::FnDecl { .. }
            | Stmt::ClassDecl { .. } => {}
        }
    }

    fn branches(&mut self, a: &Analysis<'_>, sc: &Scope, s: &Stmt) {
        match s {
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.exprs(a, sc, *cond);
                self.block(a, sc, &[then_branch]);
                if let Some(e) = else_branch {
                    self.block(a, sc, &[e]);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.exprs(a, sc, *scrutinee);
                for c in cases {
                    self.exprs(a, sc, c.value);
                }
                let arms: Vec<&Stmt> = (cases.iter().flat_map(|c| &c.body))
                    .chain(default.iter().flatten())
                    .collect();
                self.block(a, sc, &arms);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(a, sc, &body.iter().collect::<Vec<_>>());
                let catch: Vec<&Stmt> = catch_body.iter().collect();
                match catch_param {
                    Some(p) => self.bound(a, sc, p, None, &catch),
                    None => self.block(a, sc, &catch),
                }
                if let Some(f) = finally_body {
                    self.block(a, sc, &f.iter().collect::<Vec<_>>());
                }
            }
            _ => {}
        }
    }

    fn loops(&mut self, a: &Analysis<'_>, sc: &Scope, s: &Stmt) {
        match s {
            Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
                self.exprs(a, sc, *cond);
                self.block(a, sc, &[body]);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                self.frames.push(HashMap::new());
                if let Some(i) = init {
                    self.predeclare(a, sc, &[i], false);
                    self.stmt(a, sc, i);
                }
                for e in cond.iter().chain(step.iter()) {
                    self.exprs(a, sc, *e);
                }
                self.block(a, sc, &[body]);
                self.frames.pop();
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.exprs(a, sc, *parent);
                self.exprs(a, sc, *sep);
                self.bound(a, sc, var_name, None, &[body]);
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                for e in std::iter::once(elem_expr).chain(forin_obj.iter()) {
                    self.exprs(a, sc, *e);
                }
                self.bound(a, sc, var_name, Some(*elem_expr), &[body]);
            }
            _ => {}
        }
    }

    /// The assignments and closure constructions under `root`.
    fn exprs(&mut self, a: &Analysis<'_>, sc: &Scope, root: ExprId) {
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            self.container_writes(a, sc, e);
            match a.ast.get_expr(e) {
                Expr::Assign { target, value } => {
                    if let Expr::Ident(n) = a.ast.get_expr(*target) {
                        let dsts = self.resolve(a, n, sc).writes();
                        self.flow(a, &dsts, *value, sc);
                    }
                }
                Expr::Call { callee, args } => self.call_args(a, *callee, args, sc),
                Expr::Closure { fn_name, captures } => {
                    for n in captures {
                        let r = Some(self.resolve(a, n, sc));
                        let slot = (self.caps_out)
                            .entry((fn_name.clone(), n.clone()))
                            .or_insert_with(|| r.clone());
                        if *slot != r {
                            *slot = None;
                        }
                    }
                }
                _ => {}
            }
            visit_expr_children(a.ast, e, &mut |c| stack.push(c));
        }
    }
}
