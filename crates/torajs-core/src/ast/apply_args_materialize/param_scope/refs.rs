//! Where one name, read from a function's own scope, resolves to that
//! scope's binding: every `Ident` (and `new N()` site) that no inner
//! declaration claims first.
//!
//! A statement list that declares the name lexically, a loop or catch
//! that binds it, and a nested function whose parameters or own
//! declarations bind it each own their references, so the walk stops
//! there. Arrow and function-expression bodies are entered, as are the
//! bodies of nested declarations, classes (computed keys included) and
//! the top-level declarations of class expressions written inside the
//! scope, since a closure reads the binding it was written beside.
//!
//! A shape whose reads cannot be accounted for (a direct `eval`, an
//! already-lifted closure) clears `ok`, and the caller leaves the
//! scope alone.

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// The synth name the parser leaves where a class expression was
/// written; its declaration sits at the top level.
const CLASS_EXPR_PREFIX: &str = "__ClassExpr_";

pub(super) struct Refs<'a> {
    ast: &'a Ast,
    name: &'a str,
    /// Nesting depth of function bodies the walk is inside.
    depth: usize,
    pub(super) sites: Vec<ExprId>,
    /// A site sits inside a function nested in the walked code.
    pub(super) in_closure: bool,
    pub(super) ok: bool,
    seen_classes: Vec<String>,
}

impl<'a> Refs<'a> {
    pub(super) fn new(ast: &'a Ast, name: &'a str) -> Self {
        Refs {
            ast,
            name,
            depth: 0,
            sites: Vec::new(),
            in_closure: false,
            ok: true,
            seen_classes: Vec::new(),
        }
    }

    /// The statements of the scope being renamed: its own lexical
    /// declarations are the binding, so they do not stop the walk.
    pub(super) fn scope_stmts<'s>(&mut self, stmts: impl IntoIterator<Item = &'s Stmt>) {
        for s in stmts {
            self.stmt(s);
        }
    }

    pub(super) fn expr(&mut self, e: ExprId) {
        let ast = self.ast;
        match ast.get_expr(e) {
            Expr::Ident(x) => {
                if x == self.name {
                    self.site(e);
                } else if x.starts_with(CLASS_EXPR_PREFIX) && !self.seen_classes.contains(x) {
                    self.seen_classes.push(x.clone());
                    let decl = ast
                        .stmts
                        .iter()
                        .find(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == x));
                    if let Some(d) = decl {
                        self.stmt(d);
                    }
                }
            }
            Expr::New {
                class_name, args, ..
            } => {
                if class_name == self.name {
                    self.site(e);
                }
                for a in args {
                    self.expr(*a);
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                if ast.fn_expr_self_names.get(&e).map(String::as_str) != Some(self.name) {
                    self.function(params, body);
                }
            }
            Expr::Closure { .. } => self.ok = false,
            Expr::Call { callee, .. } if matches!(ast.get_expr(*callee), Expr::Ident(c) if c == "eval") =>
            {
                self.ok = false;
            }
            Expr::ObjectLit { fields } => {
                for (_, v) in fields {
                    if let Some(k) = ast.objlit_computed_keys.get(v) {
                        self.expr(*k);
                    }
                    self.expr(*v);
                }
            }
            _ => {
                for c in expr_children(ast, e) {
                    self.expr(c);
                }
            }
        }
    }

    fn site(&mut self, e: ExprId) {
        self.sites.push(e);
        if self.depth > 0 {
            self.in_closure = true;
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::FnDecl { params, body, .. } => self.function(params, body),
            Stmt::ClassDecl { .. } => self.class(s),
            Stmt::Block(v) => self.block(v),
            Stmt::Multi(v) => self.scope_stmts(v),
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                let all = cases
                    .iter()
                    .flat_map(|c| c.body.iter())
                    .chain(default.iter().flatten());
                if all.clone().any(|s| lex_declares(s, self.name)) {
                    return;
                }
                for c in cases {
                    self.expr(c.value);
                }
                self.scope_stmts(all);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(body);
                if catch_param.as_deref() != Some(self.name) {
                    self.block(catch_body);
                }
                if let Some(f) = finally_body {
                    self.block(f);
                }
            }
            Stmt::For { init, .. }
                if init.as_deref().is_some_and(|i| lex_declares(i, self.name)) => {}
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. }
                if var_name == self.name =>
            {
                for e in stmt_exprs(s) {
                    self.expr(e);
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

    fn block(&mut self, v: &[Stmt]) {
        if !v.iter().any(|s| lex_declares(s, self.name)) {
            self.scope_stmts(v);
        }
    }

    /// A nested function: a parameter of the name hides it from the
    /// whole function, a body declaration only from the body (its
    /// defaults still read this scope, §10.2.11).
    fn function(&mut self, params: &[Param], body: &[Stmt]) {
        if params.iter().any(|p| p.name == self.name) {
            return;
        }
        self.depth += 1;
        for p in params {
            if let Some(d) = p.default {
                self.expr(d);
            }
        }
        if !fn_declares(body, self.name) {
            self.scope_stmts(body);
        }
        self.depth -= 1;
    }

    fn class(&mut self, s: &Stmt) {
        let Stmt::ClassDecl {
            name,
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } = s
        else {
            return;
        };
        if let Some(p) = parent {
            self.expr(*p);
        }
        if name == self.name {
            return;
        }
        let ast = self.ast;
        let mut keys: Vec<ExprId> = ast
            .class_computed_keys
            .iter()
            .filter(|((c, _), _)| c == name)
            .map(|(_, k)| *k)
            .collect();
        keys.sort_by_key(|k| k.0);
        for k in keys {
            self.expr(k);
        }
        for (c, _, init) in &ast.class_computed_static_fields {
            if c == name {
                self.expr(*init);
            }
        }
        if let Some(c) = ctor {
            self.function(&c.params, &c.body);
        }
        for m in methods.iter().chain(static_methods.iter()) {
            self.function(&m.params, &m.body);
        }
        for si in static_init {
            match si {
                StaticInit::Field(f) => {
                    self.depth += 1;
                    self.expr(f.init);
                    self.depth -= 1;
                }
                StaticInit::Block(v) => self.function(&[], v),
            }
        }
    }
}

/// A declaration of `name` this statement adds to the list it sits in
/// (a `Multi` is a grouping, not a scope; a `for` head declares into
/// the loop).
pub(super) fn lex_declares(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::LetDecl {
            name: n, is_var, ..
        } => !is_var && n == name,
        Stmt::UsingDecl { name: n, .. }
        | Stmt::FnDecl { name: n, .. }
        | Stmt::ClassDecl { name: n, .. } => n == name,
        Stmt::Multi(v) => v.iter().any(|s| lex_declares(s, name)),
        _ => false,
    }
}

/// Whether a function body binds `name` itself: a `var` anywhere in it
/// (nested functions aside), or a top-level declaration.
pub(super) fn fn_declares(body: &[Stmt], name: &str) -> bool {
    body.iter()
        .any(|s| lex_declares(s, name) || var_declares(s, name))
}

/// A `var` of `name` in this statement or under it, nested functions
/// and classes excluded.
pub(super) fn var_declares(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::LetDecl {
            name: n,
            is_var: true,
            ..
        } => n == name,
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => false,
        _ => stmt_children_ref(s)
            .into_iter()
            .any(|c| var_declares(c, name)),
    }
}
