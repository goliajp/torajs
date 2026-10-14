//! Whether the receiver of an any-lane write can be a struct cell with
//! a slot of the written name.
//!
//! A write through a receiver the checker types `any` lands on whatever
//! cell the receiver holds. Only a struct cell whose layout has a slot
//! of that name keeps the written value where a typed read of the slot
//! looks; any other cell (a dynamic object, an array, a cell laid out
//! without that slot) takes it as a property of its own. So a write
//! whose receiver provably holds only such cells leaves every layout's
//! slot of that name as it was: `const cfg: any = {}; cfg.x = v` does
//! not touch the `x` of any struct.
//!
//! A receiver is traced through the bindings it names, by name over the
//! whole program: a binding whose every declaration initializes it, and
//! every assignment stores into it, a value of one of the shapes below.
//! A name bound any other way (a parameter, a loop or catch variable, an
//! import, a function, a class) can hold anything.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;

pub(super) struct Origins<'a> {
    ast: &'a Ast,
    expr_types: &'a HashMap<ExprId, Type>,
    /// The bindings traced so far, by name ([`Binding::of`]).
    traced: RefCell<HashMap<&'a str, Option<Vec<ExprId>>>>,
}

/// How the program binds one name: every value it is initialized or
/// assigned with, unless it is also bound some other way or stepped by
/// `++` / `--` (`opaque`), or a class declaration survived to here,
/// whose bodies are not walked, so that no binding is traced.
struct Binding<'n> {
    name: &'n str,
    values: Vec<ExprId>,
    opaque: bool,
}

impl<'a> Origins<'a> {
    pub(super) fn of(ast: &'a Ast, expr_types: &'a HashMap<ExprId, Type>) -> Self {
        Origins {
            ast,
            expr_types,
            traced: RefCell::default(),
        }
    }

    /// The values the binding `n` holds, none when it can hold anything.
    fn values(&self, n: &'a str) -> Option<Vec<ExprId>> {
        if let Some(v) = self.traced.borrow().get(n) {
            return v.clone();
        }
        let v = Binding::of(self.ast, n);
        self.traced.borrow_mut().insert(n, v.clone());
        v
    }

    /// Whether the receiver `obj` never holds a struct cell whose
    /// layout has a slot called `field`.
    pub(super) fn excludes(&self, obj: ExprId, field: &str) -> bool {
        self.value_excludes(obj, Some(field), &mut HashSet::new())
    }

    /// Whether the receiver `obj` never holds a struct cell with a slot
    /// of any name: a write under a key computed at run time cannot
    /// reach a layout through it.
    pub(super) fn holds_no_slot(&self, obj: ExprId) -> bool {
        self.value_excludes(obj, None, &mut HashSet::new())
    }

    /// `field` none: no slot of any name.
    fn value_excludes(&self, e: ExprId, field: Option<&str>, seen: &mut HashSet<&'a str>) -> bool {
        match self.ast.get_expr(e) {
            Expr::Null
            | Expr::Uninit
            | Expr::Number(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::BigInt { .. }
            | Expr::Array(_) => true,
            Expr::As { expr, ty_ann } if ty_ann == "any" => self.value_excludes(*expr, field, seen),
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                self.value_excludes(*then_branch, field, seen)
                    && self.value_excludes(*else_branch, field, seen)
            }
            Expr::Sequence { right, .. } => self.value_excludes(*right, field, seen),
            // a key the parser minted (`__computed_N__`) is computed at
            // run time and may spell `field`
            Expr::ObjectLit { fields } => {
                fields.iter().all(|(k, v)| {
                    k.as_wtf8()
                        .as_str()
                        .is_some_and(|k| field.is_some_and(|f| k != f) && !k.starts_with("__"))
                        && !matches!(self.ast.get_expr(*v), Expr::Spread { .. })
                }) && match self.expr_types.get(&e) {
                    None | Some(Type::Any | Type::Dict(..)) => true,
                    Some(Type::Struct(fs)) => {
                        fs.iter().all(|(k, _)| field.is_some_and(|f| *k != f))
                    }
                    _ => false,
                }
            }
            // the any-lane parse builds dynamic objects and arrays; a
            // reviver may hand back any cell
            Expr::Call { callee, args } => {
                args.len() == 1
                    && matches!(self.ast.get_expr(*callee), Expr::Member { obj, name }
                        if name == "parse"
                            && matches!(self.ast.get_expr(*obj), Expr::Ident(j) if j == "JSON"))
            }
            Expr::Ident(n) if n == "undefined" => true,
            Expr::Ident(n) => {
                let Some(vs) = self.values(n) else {
                    return false;
                };
                if !seen.insert(n.as_str()) {
                    return true;
                }
                !vs.is_empty() && vs.iter().all(|v| self.value_excludes(*v, field, seen))
            }
            _ => false,
        }
    }
}

impl<'n> Binding<'n> {
    fn of(ast: &Ast, name: &'n str) -> Option<Vec<ExprId>> {
        let mut o = Binding {
            name,
            values: Vec::new(),
            opaque: false,
        };
        o.stmts(&ast.stmts);
        for e in &ast.exprs {
            if o.opaque {
                return None;
            }
            match e {
                Expr::Assign { target, value } => {
                    if let Expr::Ident(n) = ast.get_expr(*target) {
                        o.value(n, *value);
                    }
                }
                Expr::PostIncr { target, .. } => {
                    if let Expr::Ident(n) = ast.get_expr(*target) {
                        o.bound(n);
                    }
                }
                Expr::ArrowFn { params, body, .. } => {
                    o.bound_all(params.iter().map(|p| p.name.as_str()));
                    o.stmts(body);
                }
                _ => {}
            }
        }
        (!o.opaque).then_some(o.values)
    }

    fn value(&mut self, n: &str, v: ExprId) {
        if n == self.name {
            self.values.push(v);
        }
    }

    fn bound(&mut self, n: &str) {
        self.opaque |= n == self.name;
    }

    fn bound_all<'s>(&mut self, ns: impl IntoIterator<Item = &'s str>) {
        for n in ns {
            self.bound(n);
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        if self.opaque {
            return;
        }
        match s {
            Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
                self.value(name, *init);
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                self.bound(name);
                self.bound_all(params.iter().map(|p| p.name.as_str()));
                self.stmts(body);
            }
            Stmt::ForOf {
                var_name,
                src_ident,
                i_ident,
                body,
                ..
            } => {
                self.bound_all([var_name, src_ident, i_ident].map(String::as_str));
                self.stmt(body);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                self.bound(var_name);
                self.stmt(body);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.bound_all(catch_param.iter().map(String::as_str));
                self.stmts(body);
                self.stmts(catch_body);
                self.stmts(finally_body.as_deref().unwrap_or(&[]));
            }
            Stmt::YieldInto { var, .. } => {
                self.bound(var);
            }
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                type_named,
                ..
            } => {
                self.bound_all(default.iter().map(String::as_str));
                self.bound_all(namespace.iter().map(String::as_str));
                for (n, alias) in named.iter().chain(type_named) {
                    self.bound(alias.as_deref().unwrap_or(n));
                }
            }
            Stmt::ClassDecl { .. } => {
                // its bodies are not walked: no binding is traced
                self.opaque = true;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.stmt(then_branch);
                if let Some(e) = else_branch {
                    self.stmt(e);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                self.stmt(body)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.stmt(body);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    self.stmts(&c.body);
                }
                self.stmts(default.as_deref().unwrap_or(&[]));
            }
            Stmt::Block(ss) | Stmt::Multi(ss) => self.stmts(ss),
            Stmt::ExportDecl { inner, .. } => {
                if let Some(s) = inner {
                    self.stmt(s);
                }
            }
            Stmt::Expr(_)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::Throw(_)
            | Stmt::Return(_)
            | Stmt::Yield(_)
            | Stmt::TypeDecl { .. } => {}
        }
    }
}
