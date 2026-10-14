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

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;

pub(super) struct Origins<'a> {
    ast: &'a Ast,
    expr_types: &'a HashMap<ExprId, Type>,
    /// Every value each binding name is initialized or assigned with.
    values: HashMap<String, Vec<ExprId>>,
    /// The names bound some other way, or stepped by `++` / `--`.
    opaque: HashSet<String>,
    /// A class declaration survived to here: its bodies are not walked,
    /// so no binding is traced.
    untraced: bool,
}

impl<'a> Origins<'a> {
    pub(super) fn of(ast: &'a Ast, expr_types: &'a HashMap<ExprId, Type>) -> Self {
        let mut o = Origins {
            ast,
            expr_types,
            values: HashMap::new(),
            opaque: HashSet::new(),
            untraced: false,
        };
        o.stmts(&ast.stmts);
        for e in &ast.exprs {
            match e {
                Expr::Assign { target, value } => {
                    if let Expr::Ident(n) = ast.get_expr(*target) {
                        o.values.entry(n.clone()).or_default().push(*value);
                    }
                }
                Expr::PostIncr { target, .. } => {
                    if let Expr::Ident(n) = ast.get_expr(*target) {
                        o.opaque.insert(n.clone());
                    }
                }
                Expr::ArrowFn { params, body, .. } => {
                    o.opaque.extend(params.iter().map(|p| p.name.clone()));
                    o.stmts(body);
                }
                _ => {}
            }
        }
        o
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
    fn value_excludes(&self, e: ExprId, field: Option<&str>, seen: &mut HashSet<String>) -> bool {
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
                if self.untraced || self.opaque.contains(n) {
                    return false;
                }
                if !seen.insert(n.clone()) {
                    return true;
                }
                self.values
                    .get(n)
                    .is_some_and(|vs| vs.iter().all(|v| self.value_excludes(*v, field, seen)))
            }
            _ => false,
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
                self.values.entry(name.clone()).or_default().push(*init);
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                self.opaque.insert(name.clone());
                self.opaque.extend(params.iter().map(|p| p.name.clone()));
                self.stmts(body);
            }
            Stmt::ForOf {
                var_name,
                src_ident,
                i_ident,
                body,
                ..
            } => {
                self.opaque
                    .extend([var_name.clone(), src_ident.clone(), i_ident.clone()]);
                self.stmt(body);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                self.opaque.insert(var_name.clone());
                self.stmt(body);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.opaque.extend(catch_param.iter().cloned());
                self.stmts(body);
                self.stmts(catch_body);
                self.stmts(finally_body.as_deref().unwrap_or(&[]));
            }
            Stmt::YieldInto { var, .. } => {
                self.opaque.insert(var.clone());
            }
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                type_named,
                ..
            } => {
                self.opaque.extend(default.iter().cloned());
                self.opaque.extend(namespace.iter().cloned());
                for (n, alias) in named.iter().chain(type_named) {
                    self.opaque
                        .insert(alias.clone().unwrap_or_else(|| n.clone()));
                }
            }
            Stmt::ClassDecl { name, .. } => {
                self.opaque.insert(name.clone());
                self.untraced = true;
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
