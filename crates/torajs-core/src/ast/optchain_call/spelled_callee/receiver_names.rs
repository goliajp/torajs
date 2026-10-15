//! The bindings a function expression reading `this` is bound to.
//!
//! The spelled-callee rewrite exists for those functions only: every
//! other callee already gets an `undefined` receiver through the
//! plain call path, and a call through a conditional keeps the
//! union-typed call (`(c ? inc : echo)("s")` checks the argument
//! against the union, where two branch calls would each check it
//! against one signature and refuse what the union admits).
//!
//! A leaf name resolves along the chain of function bodies around it
//! (the program, function declarations, function expressions and
//! arrows, class members) to the nearest one declaring it, so a `const
//! f` in another function does not count. Block scopes inside one body
//! are not told apart: a second binding of the name there only makes
//! the rewrite run where it was not needed. A leaf the walk does not
//! reach is judged by its name alone.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

pub(super) struct Receivers {
    /// The body each reached expression sits in.
    unit_of: HashMap<ExprId, usize>,
    units: Vec<Unit>,
    /// Bindings holding such a function: the declaring body, `None`
    /// for a name no body declares.
    held: HashSet<(Option<usize>, String)>,
    names: HashSet<String>,
}

struct Unit {
    parent: Option<usize>,
    decls: HashSet<String>,
}

impl Receivers {
    pub(super) fn of(ast: &Ast) -> Self {
        let mut b = Build {
            ast,
            units: Vec::new(),
            unit_of: HashMap::new(),
            writes: Vec::new(),
        };
        b.unit(None, &[], &ast.stmts);
        let mut out = Receivers {
            unit_of: b.unit_of,
            units: b.units,
            held: HashSet::new(),
            names: HashSet::new(),
        };
        for (u, name) in b.writes {
            let at = out.resolve(u, &name);
            out.names.insert(name.clone());
            out.held.insert((at, name));
        }
        out
    }

    /// `e` is a name whose binding holds a function expression reading
    /// `this`.
    pub(super) fn names(&self, ast: &Ast, e: ExprId) -> bool {
        let Expr::Ident(n) = ast.get_expr(e) else {
            return false;
        };
        match self.unit_of.get(&e) {
            Some(&u) => self.held.contains(&(self.resolve(u, n), n.clone())),
            None => self.names.contains(n),
        }
    }

    fn resolve(&self, mut u: usize, name: &str) -> Option<usize> {
        loop {
            if self.units[u].decls.contains(name) {
                return Some(u);
            }
            u = self.units[u].parent?;
        }
    }
}

struct Build<'a> {
    ast: &'a Ast,
    units: Vec<Unit>,
    unit_of: HashMap<ExprId, usize>,
    /// A name given such a function, and the body the write sits in.
    writes: Vec<(usize, String)>,
}

impl Build<'_> {
    fn unit(&mut self, parent: Option<usize>, params: &[Param], body: &[Stmt]) {
        let u = self.units.len();
        self.units.push(Unit {
            parent,
            decls: params.iter().map(|p| p.name.clone()).collect(),
        });
        for d in params.iter().filter_map(|p| p.default) {
            self.expr(u, d);
        }
        for s in body {
            self.stmt(u, s);
        }
    }

    fn declare(&mut self, u: usize, name: &str) {
        self.units[u].decls.insert(name.to_string());
    }

    fn stmt(&mut self, u: usize, s: &Stmt) {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                self.declare(u, name);
                return self.unit(Some(u), params, body);
            }
            Stmt::ClassDecl {
                name,
                parent,
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                self.declare(u, name);
                if let Some(p) = parent {
                    self.expr(u, *p);
                }
                if let Some(c) = ctor {
                    self.unit(Some(u), &c.params, &c.body);
                }
                for m in methods.iter().chain(static_methods) {
                    self.unit(Some(u), &m.params, &m.body);
                }
                for i in static_init {
                    match i {
                        StaticInit::Block(b) => self.unit(Some(u), &[], b),
                        StaticInit::Field(f) => self.expr(u, f.init),
                    }
                }
                return;
            }
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    self.stmt(u, i);
                }
                return;
            }
            Stmt::LetDecl { name, init, .. } => {
                self.declare(u, name);
                if reads_this_fnexpr(self.ast, *init) {
                    self.writes.push((u, name.clone()));
                }
            }
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => self.declare(u, p),
            Stmt::ForOf { var_name, .. } => self.declare(u, var_name),
            _ => {}
        }
        for e in stmt_exprs(s) {
            self.expr(u, e);
        }
        for c in stmt_children_ref(s) {
            self.stmt(u, c);
        }
    }

    fn expr(&mut self, u: usize, root: ExprId) {
        let mut todo = vec![root];
        while let Some(e) = todo.pop() {
            self.unit_of.insert(e, u);
            match self.ast.get_expr(e) {
                Expr::ArrowFn { params, body, .. } => {
                    self.unit(Some(u), params, body);
                    continue;
                }
                Expr::Assign { target, value } => {
                    if let Expr::Ident(name) = self.ast.get_expr(*target)
                        && reads_this_fnexpr(self.ast, *value)
                    {
                        self.writes.push((u, name.clone()));
                    }
                }
                _ => {}
            }
            todo.extend(expr_children(self.ast, e));
        }
    }
}

fn reads_this_fnexpr(ast: &Ast, mut e: ExprId) -> bool {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    match ast.get_expr(e) {
        Expr::ArrowFn { body, .. } if ast.fn_expr_exprs.contains(&e) => body_reads_this(ast, body),
        _ => false,
    }
}

/// `this` in the body itself or in an arrow nested in it (§10.2.1.2:
/// an arrow takes the enclosing `this`); a nested function or class
/// binds its own.
fn body_reads_this(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_reads_this(ast, s))
}

fn stmt_reads_this(ast: &Ast, s: &Stmt) -> bool {
    stmt_exprs(s).into_iter().any(|e| expr_reads_this(ast, e))
        || stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_reads_this(ast, c))
}

fn expr_reads_this(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::This => true,
        Expr::Ident(n) => n == "__this",
        Expr::ArrowFn { body, .. } => !ast.fn_expr_exprs.contains(&e) && body_reads_this(ast, body),
        _ => expr_children(ast, e)
            .into_iter()
            .any(|c| expr_reads_this(ast, c)),
    }
}
