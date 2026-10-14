//! Which binding a name resolves to at each `globalThis.X` read, for
//! the names those reads spell.
//!
//! `globalThis.X` is a property read of the global object, so no scope
//! between the read and the global environment changes what it reads
//! (§9.4.2). Rewriting it to the bare `X` is only right where the bare
//! name would also reach the global object: where no declaration of
//! `X` is in scope. This walk answers that per read, and records for
//! every bare `X` in the program which side it reaches, so a binding
//! that is in scope at some read can be renamed out of the way.
//!
//! Scopes follow §9.1 closely enough to rename by: a function (its
//! parameters, its `var`s anywhere in its body, the lexical
//! declarations of its body), a block (its own `let` / `const` /
//! `using`), a `for` head, a catch clause, a class body (its name). A
//! function or class declaration and a loop variable are put in the
//! enclosing function's scope wherever they sit, which can only make a
//! read look shadowed when it is not; those binders are never renamed
//! (`pinned`), so the over-approximation keeps a read on the member
//! form and changes nothing else. The top-level frame drops the names
//! `take_census`'s `props` lists, so a `for…of` / `for…in` body binds
//! its loop variable in a frame of its own as well.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_node_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// What the walk learned about the watched names.
#[derive(Default)]
pub(super) struct Census {
    /// Each `globalThis.X` read the walk reached: whether a
    /// declaration of `X` is in scope there.
    pub(super) site_shadowed: HashMap<ExprId, bool>,
    /// The bare `X` occurrences (`Ident`, `new X`) that reach a
    /// declaration rather than the global object.
    pub(super) user_refs: HashMap<String, Vec<ExprId>>,
    /// Every bare occurrence of a watched name the walk reached.
    pub(super) reached_refs: HashSet<ExprId>,
    /// Names with a declaration whose rename would be observable (a
    /// function's or class's `name`, a named-evaluation target) or
    /// whose extent the scopes above only approximate.
    pub(super) pinned: HashSet<String>,
    /// How many arrow / function-expression nodes the walk entered.
    pub(super) arrows_reached: usize,
}

/// `props`: the names whose top-level declaration is the global
/// object's own property (`script_global_props`) — reaching that
/// declaration IS reaching the global object, so it shadows nothing.
pub(super) fn take_census(ast: &Ast, watch: &HashSet<String>, props: &HashSet<String>) -> Census {
    let mut w = Walk {
        ast,
        watch,
        frames: Vec::new(),
        census: Census::default(),
    };
    let mut top = w.fn_scope(&[], &ast.stmts, None);
    top.retain(|n| !props.contains(n));
    w.frames.push(top);
    w.stmts(&ast.stmts);
    w.census
}

/// The names a sloppy script's top level turns into own properties of
/// the global object (§16.1.7): its `var`s and its function
/// declarations. `globalThis.X` reads the program's own value for
/// those, and so does a bare `X` that reaches the declaration.
pub(super) fn script_global_props(ast: &Ast, watch: &HashSet<String>) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_vars(&ast.stmts, watch, &mut out);
    out
}

fn collect_vars(stmts: &[Stmt], watch: &HashSet<String>, out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name, is_var: true, ..
            }
            | Stmt::FnDecl { name, .. }
                if watch.contains(name) =>
            {
                out.insert(name.clone());
            }
            _ => {}
        }
        collect_vars_children(s, watch, out);
    }
}

fn collect_vars_children(s: &Stmt, watch: &HashSet<String>, out: &mut HashSet<String>) {
    for c in stmt_children_ref(s) {
        collect_vars(std::slice::from_ref(c), watch, out);
    }
}

struct Walk<'a> {
    ast: &'a Ast,
    watch: &'a HashSet<String>,
    /// Innermost last; each holds only watched names.
    frames: Vec<Vec<String>>,
    census: Census,
}

impl Walk<'_> {
    fn in_scope(&self, n: &str) -> bool {
        self.frames.iter().any(|f| f.iter().any(|x| x == n))
    }

    fn bind(&mut self, out: &mut Vec<String>, name: &str, pin: bool) {
        if self.watch.contains(name) {
            out.push(name.to_string());
            if pin {
                self.census.pinned.insert(name.to_string());
            }
        }
    }

    /// A function's scope: parameters, its own name when it is a named
    /// function expression, and every declaration of its body.
    fn fn_scope(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        self_name: Option<&str>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for p in params {
            self.bind(&mut out, &p.name, false);
        }
        if let Some(n) = self_name {
            self.bind(&mut out, n, true);
        }
        self.fn_binders(body, true, &mut out);
        out
    }

    /// `top`: the statement sits directly in the function body, where a
    /// lexical declaration is the function scope's own.
    fn fn_binders(&mut self, stmts: &[Stmt], top: bool, out: &mut Vec<String>) {
        for s in stmts {
            match s {
                Stmt::LetDecl {
                    name, is_var, init, ..
                } => {
                    if *is_var || top {
                        let named_eval = self.names_its_value(*init);
                        self.bind(out, name, named_eval);
                    }
                }
                Stmt::UsingDecl { name, .. } if top => self.bind(out, name, false),
                Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
                    self.bind(out, name, true);
                    continue;
                }
                Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                    self.bind(out, var_name, true)
                }
                Stmt::ImportDecl {
                    default,
                    namespace,
                    named,
                    ..
                } => {
                    for n in default.iter().chain(namespace.iter()) {
                        self.bind(out, n, true);
                    }
                    for (n, alias) in named {
                        self.bind(out, alias.as_ref().unwrap_or(n), true);
                    }
                }
                Stmt::ExportDecl { named, .. } => {
                    for (n, _) in named {
                        if self.watch.contains(n) {
                            self.census.pinned.insert(n.clone());
                        }
                    }
                }
                Stmt::Multi(v) => {
                    self.fn_binders(v, top, out);
                    continue;
                }
                _ => {}
            }
            if let Stmt::ExportDecl {
                inner: Some(inner), ..
            } = s
            {
                self.fn_binders(std::slice::from_ref(inner), top, out);
            }
            for c in stmt_children_ref(s) {
                self.fn_binders(std::slice::from_ref(c), false, out);
            }
        }
    }

    /// §8.4.5 NamedEvaluation: the initializer is an anonymous
    /// function, so the binding's name becomes the function's `name`.
    fn names_its_value(&self, init: ExprId) -> bool {
        match self.ast.get_expr(init) {
            Expr::As { expr, .. } => self.names_its_value(*expr),
            Expr::ArrowFn { .. } => true,
            _ => false,
        }
    }

    /// A block's own lexical declarations.
    fn block_scope(&mut self, stmts: &[Stmt]) -> Vec<String> {
        let mut out = Vec::new();
        for s in stmts {
            match s {
                Stmt::LetDecl {
                    name,
                    is_var: false,
                    init,
                    ..
                } => {
                    let named_eval = self.names_its_value(*init);
                    self.bind(&mut out, name, named_eval);
                }
                Stmt::UsingDecl { name, .. } => self.bind(&mut out, name, false),
                Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
                    self.bind(&mut out, name, true)
                }
                Stmt::Multi(v) => out.extend(self.block_scope(v)),
                _ => {}
            }
        }
        out
    }

    fn scoped(&mut self, frame: Vec<String>, f: impl FnOnce(&mut Self)) {
        self.frames.push(frame);
        f(self);
        self.frames.pop();
    }

    fn block(&mut self, stmts: &[Stmt]) {
        let frame = self.block_scope(stmts);
        self.scoped(frame, |w| w.stmts(stmts));
    }

    fn function(&mut self, params: &[Param], body: &[Stmt], self_name: Option<&str>) {
        // §10.2.11: with parameter expressions present, the body's own
        // declarations are not in scope for the defaults
        let mut head = Vec::new();
        for p in params {
            self.bind(&mut head, &p.name, false);
        }
        if let Some(n) = self_name {
            self.bind(&mut head, n, true);
        }
        self.scoped(head, |w| {
            for d in params.iter().filter_map(|p| p.default) {
                w.expr(d);
            }
        });
        let frame = self.fn_scope(params, body, self_name);
        self.scoped(frame, |w| w.stmts(body));
    }

    /// The loop variable of a `for…of` / `for…in` is the body's own
    /// binding (§14.7.5.7 — a `var` head names a fresh loop local
    /// here), whatever the enclosing scope's frame says about the name.
    fn loop_body(&mut self, var_name: &str, body: &Stmt) {
        let mut frame = Vec::new();
        self.bind(&mut frame, var_name, true);
        self.scoped(frame, |w| w.stmt(body));
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(v) => self.block(v),
            Stmt::Multi(v) => self.stmts(v),
            Stmt::FnDecl { params, body, .. } => self.function(params, body, None),
            Stmt::ClassDecl {
                name,
                parent,
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                let mut frame = Vec::new();
                self.bind(&mut frame, name, true);
                self.scoped(frame, |w| {
                    if let Some(p) = parent {
                        w.expr(*p);
                    }
                    if let Some(c) = ctor {
                        w.function(&c.params, &c.body, None);
                    }
                    for m in methods.iter().chain(static_methods.iter()) {
                        w.function(&m.params, &m.body, None);
                    }
                    for si in static_init {
                        match si {
                            StaticInit::Block(b) => w.function(&[], b, None),
                            StaticInit::Field(f) => w.expr(f.init),
                        }
                    }
                });
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let frame = match init {
                    Some(i) => self.block_scope(std::slice::from_ref(i)),
                    None => Vec::new(),
                };
                self.scoped(frame, |w| {
                    if let Some(i) = init {
                        w.stmt(i);
                    }
                    for e in cond.iter().chain(step.iter()) {
                        w.expr(*e);
                    }
                    w.stmt(body);
                });
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                self.expr(*elem_expr);
                if let Some(o) = forin_obj {
                    self.expr(*o);
                }
                self.loop_body(var_name, body);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.loop_body(var_name, body);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(body);
                let mut frame = Vec::new();
                if let Some(p) = catch_param {
                    self.bind(&mut frame, p, false);
                }
                self.scoped(frame, |w| w.block(catch_body));
                if let Some(f) = finally_body {
                    self.block(f);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // the clauses share one block (§14.12.4)
                let mut frame = Vec::new();
                for body in cases.iter().map(|c| &c.body).chain(default.iter()) {
                    frame.extend(self.block_scope(body));
                }
                self.scoped(frame, |w| {
                    for c in cases {
                        w.expr(c.value);
                        w.stmts(&c.body);
                    }
                    if let Some(d) = default {
                        w.stmts(d);
                    }
                });
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
            _ => {
                for e in stmt_exprs(s) {
                    self.expr(e);
                }
                for c in stmt_children_ref(s) {
                    self.stmt(c);
                }
            }
        }
    }

    fn expr(&mut self, eid: ExprId) {
        let ast = self.ast;
        match ast.get_expr(eid) {
            Expr::Ident(n) | Expr::New { class_name: n, .. } if self.watch.contains(n) => {
                self.census.reached_refs.insert(eid);
                if self.in_scope(n) {
                    self.census
                        .user_refs
                        .entry(n.clone())
                        .or_default()
                        .push(eid);
                }
            }
            Expr::Member { obj, name }
                if self.watch.contains(name)
                    && matches!(ast.get_expr(*obj), Expr::Ident(g) if g == "globalThis") =>
            {
                let shadowed = self.in_scope(name);
                self.census.site_shadowed.insert(eid, shadowed);
                return;
            }
            Expr::ArrowFn { params, body, .. } => {
                self.census.arrows_reached += 1;
                let self_name = ast.fn_expr_self_names.get(&eid).map(String::as_str);
                self.function(params, body, self_name);
                return;
            }
            Expr::Closure { captures, .. } => {
                for c in captures.iter().filter(|c| self.watch.contains(*c)) {
                    self.census.pinned.insert(c.clone());
                }
            }
            _ => {}
        }
        for c in expr_node_children(ast.get_expr(eid)) {
            self.expr(c);
        }
    }
}
