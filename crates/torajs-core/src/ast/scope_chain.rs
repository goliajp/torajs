//! Which names the scopes around a nested site bind.
//!
//! Several passes move a nested function or class to the top level
//! when nothing it reads belongs to the scopes around it, and decide
//! that by pre-binding the top-level names: a free name that is also
//! declared at the top level reads as resolved. That is right until a
//! scope in between declares the same name — the reference then
//! belongs to that binding, and the moved code silently reads the
//! top-level one (`const k = 1; function g(k) { class D { m() { return
//! k } } }` answered 1). The question each of them has to ask is
//! whether a frame between the site and the top level binds the name,
//! and this file is the one answer.
//!
//! A frame binds: a function's parameters, its self-name and every
//! `var` in its body; a statement list's lexical, function and class
//! declarations for its whole extent (a reference before the
//! declaration is that binding's dead zone, never the outer one); a
//! `for` head's declarations; a `for-of` variable; a catch parameter.
//! The top-level list itself is never a frame here.
//!
//! The walk that answers for function expressions and arrows reaches
//! every expression a statement holds, class members included. A
//! function in a side table (a computed class key) is not reached and
//! reads as having no frame around it.

use super::desugar_async_sm_walk::visit_expr_children;
use super::free_vars_hoisted_names::hoist_var_names;
use super::{Ast, Expr, ExprId, Param, StaticInit, Stmt};
use std::collections::HashMap;

/// The names `list` binds for its whole extent: its lexical, function
/// and class declarations. A compiler-minted `Multi` shares the list's
/// scope (a destructuring expansion's lets), and so does the try body
/// the `using` desugar wraps the rest of the list in.
pub(super) fn list_names(list: &[Stmt], out: &mut Vec<String>) {
    names_of(list, true, out);
}

/// [`list_names`] without the class declarations — for the nested
/// class hoist, which treats a container's classes as one another's
/// siblings rather than as bindings around them.
pub(super) fn list_names_but_classes(list: &[Stmt], out: &mut Vec<String>) {
    names_of(list, false, out);
}

fn names_of(list: &[Stmt], classes: bool, out: &mut Vec<String>) {
    for s in list {
        match s {
            Stmt::ClassDecl { name, .. } if classes => out.push(name.clone()),
            Stmt::FnDecl { name, .. }
            | Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. } => out.push(name.clone()),
            Stmt::Multi(v) => names_of(v, classes, out),
            Stmt::Try {
                body,
                catch_param: Some(p),
                ..
            } if p.starts_with("__usc_") => names_of(body, classes, out),
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => names_of(std::slice::from_ref(inner), classes, out),
            _ => {}
        }
    }
}

/// The names a function's own frame binds around its body list: the
/// parameters and every `var` the body declares, however deep in
/// blocks (the body list's own declarations are [`list_names`]').
pub(super) fn fn_names(params: &[Param], body: &[Stmt], out: &mut Vec<String>) {
    out.extend(params.iter().map(|p| p.name.clone()));
    hoist_var_names(body, out);
}

/// The names a `for` head declares, in scope for the head and body.
pub(super) fn for_init_names(init: Option<&Stmt>, out: &mut Vec<String>) {
    if let Some(i) = init {
        list_names(std::slice::from_ref(i), out);
    }
}

/// Function expression / arrow → the names the frames around it bind,
/// for every one reachable from `roots` (each a top-level statement).
/// An absent key means no frame around it binds anything.
pub(super) fn chains_by_arrow(ast: &Ast, roots: &[&[Stmt]]) -> HashMap<ExprId, Vec<String>> {
    walk(ast, roots).out
}

/// What a function expression's body list sits inside: the frames
/// around it (`around`, from [`chains_by_arrow`]), its self-name, its
/// parameters and its body's `var`s.
pub(super) fn arrow_frames(
    ast: &Ast,
    around: Option<&Vec<String>>,
    eid: ExprId,
    body: &[Stmt],
) -> Vec<String> {
    let mut out = around.cloned().unwrap_or_default();
    out.extend(ast.fn_expr_self_names.get(&eid).cloned());
    if let Expr::ArrowFn { params, .. } = &ast.exprs[eid.0 as usize] {
        fn_names(params, body, &mut out);
    }
    out
}

/// A nested function declaration, by name and source range.
pub(super) type DeclKey = (String, u32, u32);

/// Nested function declaration → the names the frames around it bind
/// (its own list's declarations included). A declaration a body clone
/// repeats is one key, holding what every copy's frames bind.
pub(super) fn chains_by_fn_decl(ast: &Ast, roots: &[&[Stmt]]) -> HashMap<DeclKey, Vec<String>> {
    walk(ast, roots).decls
}

fn walk<'a>(ast: &'a Ast, roots: &[&[Stmt]]) -> Walk<'a> {
    let mut w = Walk {
        ast,
        chain: Vec::new(),
        out: HashMap::new(),
        decls: HashMap::new(),
    };
    for list in roots {
        for s in list.iter() {
            w.stmt(s);
        }
    }
    w
}

/// `globals` less the names in `shadow`.
pub(super) fn not_rebound(globals: &[String], shadow: &[String]) -> Vec<String> {
    globals
        .iter()
        .filter(|g| !shadow.contains(g))
        .cloned()
        .collect()
}

/// The subset of `globals` the frames around each function expression
/// bind — the names `lift_arrow_fns` must not pre-bind for it.
pub(super) fn shadowed_globals_by_arrow(
    ast: &Ast,
    globals: &[String],
) -> HashMap<ExprId, Vec<String>> {
    let mut out = chains_by_arrow(ast, &[&ast.stmts]);
    out.retain(|_, chain| {
        let mut hit: Vec<String> = globals
            .iter()
            .filter(|g| chain.contains(g))
            .cloned()
            .collect();
        std::mem::swap(chain, &mut hit);
        !chain.is_empty()
    });
    out
}

struct Walk<'a> {
    ast: &'a Ast,
    chain: Vec<String>,
    out: HashMap<ExprId, Vec<String>>,
    decls: HashMap<DeclKey, Vec<String>>,
}

impl Walk<'_> {
    fn list(&mut self, list: &[Stmt]) {
        let depth = self.chain.len();
        list_names(list, &mut self.chain);
        for s in list {
            self.stmt(s);
        }
        self.chain.truncate(depth);
    }

    /// A single-statement body (`if`, loop, label): a block opens a
    /// frame, anything else declares nothing it could outlive.
    fn body(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(v) => self.list(v),
            _ => self.stmt(s),
        }
    }

    fn function(&mut self, params: &[Param], body: &[Stmt]) {
        let depth = self.chain.len();
        fn_names(params, body, &mut self.chain);
        for d in params.iter().filter_map(|p| p.default) {
            self.expr(d);
        }
        self.list(body);
        self.chain.truncate(depth);
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => self.expr(*e),
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => self.expr(*init),
            Stmt::YieldInto { value, .. } => self.expr(*value),
            Stmt::Return(v) => {
                if let Some(e) = v {
                    self.expr(*e);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.body(then_branch);
                if let Some(b) = else_branch {
                    self.body(b);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.expr(*cond);
                self.body(body);
            }
            Stmt::Labeled { body, .. } => self.body(body),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let depth = self.chain.len();
                for_init_names(init.as_deref(), &mut self.chain);
                if let Some(i) = init {
                    self.stmt(i);
                }
                for e in cond.iter().chain(step.iter()) {
                    self.expr(*e);
                }
                self.body(body);
                self.chain.truncate(depth);
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
                let depth = self.chain.len();
                self.chain.push(var_name.clone());
                self.body(body);
                self.chain.truncate(depth);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent);
                self.expr(*sep);
                let depth = self.chain.len();
                self.chain.push(var_name.clone());
                self.body(body);
                self.chain.truncate(depth);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // One block holds every clause.
                let depth = self.chain.len();
                for c in cases {
                    list_names(&c.body, &mut self.chain);
                }
                if let Some(d) = default {
                    list_names(d, &mut self.chain);
                }
                for c in cases {
                    self.expr(c.value);
                    c.body.iter().for_each(|s| self.stmt(s));
                }
                if let Some(d) = default {
                    d.iter().for_each(|s| self.stmt(s));
                }
                self.chain.truncate(depth);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.list(body);
                let depth = self.chain.len();
                if let Some(cp) = catch_param {
                    self.chain.push(cp.clone());
                }
                self.list(catch_body);
                self.chain.truncate(depth);
                if let Some(fb) = finally_body {
                    self.list(fb);
                }
            }
            Stmt::Block(v) => self.list(v),
            // Shares the enclosing list's scope; its names came with it.
            Stmt::Multi(v) => v.iter().for_each(|s| self.stmt(s)),
            Stmt::FnDecl {
                name,
                params,
                body,
                span,
                ..
            } => {
                if !self.chain.is_empty() {
                    let key = (name.clone(), span.start, span.end);
                    let seen = self.decls.entry(key).or_default();
                    let add: Vec<String> = self
                        .chain
                        .iter()
                        .filter(|n| !seen.contains(n))
                        .cloned()
                        .collect();
                    seen.extend(add);
                }
                self.function(params, body)
            }
            Stmt::ClassDecl {
                parent,
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(p) = parent {
                    self.expr(*p);
                }
                if let Some(c) = ctor {
                    self.function(&c.params, &c.body);
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    self.function(&m.params, &m.body);
                }
                for si in static_init {
                    match si {
                        StaticInit::Field(f) => self.expr(f.init),
                        StaticInit::Block(v) => self.function(&[], v),
                    }
                }
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
            Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::TypeDecl { .. }
            | Stmt::ImportDecl { .. } => {}
        }
    }

    fn expr(&mut self, eid: ExprId) {
        let ast = self.ast;
        if let Expr::ArrowFn { params, body, .. } = ast.get_expr(eid) {
            if !self.chain.is_empty() {
                self.out.insert(eid, self.chain.clone());
            }
            let depth = self.chain.len();
            // §15.5.5 — a named function expression's own name binds
            // in a frame between it and its body.
            if let Some(sn) = ast.fn_expr_self_names.get(&eid) {
                self.chain.push(sn.clone());
            }
            self.function(params, body);
            self.chain.truncate(depth);
            return;
        }
        visit_expr_children(ast, eid, &mut |c| self.expr(c));
    }
}
