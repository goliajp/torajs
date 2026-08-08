//! The stmt / expr walkers of the cross-type-widen scope simulation
//! — mirror of `dynobj_degrade/walk.rs` with a single trigger scan
//! (ident assigns). Child module: reaches the parent's private
//! `Walker` fields directly (dynobj_degrade/walk.rs precedent).

use super::Walker;
use crate::ast::{Expr, ExprId, Param, StaticInit, Stmt};
use std::collections::HashMap;

impl Walker<'_> {
    pub(super) fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => self.walk_expr(*e),
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                ..
            } => {
                self.walk_expr(*init);
                self.register_let(name, *mutable, type_ann.is_none(), *init);
            }
            // `using` is a const-natured binding (RFC 20260809); it
            // only reaches this walker if desugar_using was skipped,
            // and the conservative reading is identical to `const`.
            Stmt::UsingDecl {
                name,
                type_ann,
                init,
            } => {
                self.walk_expr(*init);
                self.register_let(name, false, type_ann.is_none(), *init);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond);
                self.walk_stmt(then_branch);
                if let Some(e) = else_branch {
                    self.walk_stmt(e);
                }
            }
            Stmt::While { cond, body } => {
                self.walk_expr(*cond);
                self.walk_stmt(body);
            }
            Stmt::DoWhile { body, cond } => {
                self.walk_stmt(body);
                self.walk_expr(*cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.walk_expr(*scrutinee);
                self.scopes.push(HashMap::new());
                for c in cases {
                    self.walk_expr(c.value);
                    for s in &c.body {
                        self.walk_stmt(s);
                    }
                }
                if let Some(d) = default {
                    for s in d {
                        self.walk_stmt(s);
                    }
                }
                self.scopes.pop();
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                self.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.walk_stmt(i);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c);
                }
                if let Some(st) = step {
                    self.walk_expr(*st);
                }
                self.walk_stmt(body);
                self.scopes.pop();
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.walk_expr(*parent);
                self.walk_expr(*sep);
                self.scopes.push(HashMap::new());
                self.shadow(var_name);
                self.walk_stmt(body);
                self.scopes.pop();
            }
            Stmt::ForOf {
                var_name,
                src_ident,
                i_ident,
                body,
                forin_obj,
                ..
            } => {
                if let Some(o) = forin_obj {
                    self.walk_expr(*o);
                }
                self.scopes.push(HashMap::new());
                self.shadow(var_name);
                self.shadow(src_ident);
                self.shadow(i_ident);
                self.walk_stmt(body);
                self.scopes.pop();
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Labeled { body, .. } => self.walk_stmt(body),
            Stmt::Throw(e) => self.walk_expr(*e),
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => self.walk_try(
                body,
                catch_param.as_deref(),
                catch_body,
                finally_body.as_deref(),
            ),
            Stmt::Block(v) => {
                self.scopes.push(HashMap::new());
                for s in v {
                    self.walk_stmt(s);
                }
                self.scopes.pop();
            }
            Stmt::Multi(v) => {
                for s in v {
                    self.walk_stmt(s);
                }
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                self.shadow(name);
                self.walk_fn_body(params, body);
            }
            Stmt::TypeDecl { .. } => {}
            Stmt::ClassDecl {
                static_init,
                ctor,
                methods,
                static_methods,
                ..
            } => self.walk_class_decl(static_init, ctor.as_ref(), methods, static_methods),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.walk_expr(*e);
                }
            }
            Stmt::Yield(e) => self.walk_expr(*e),
            Stmt::YieldInto { var, value, .. } => {
                self.walk_expr(*value);
                self.shadow(var);
            }
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                ..
            } => {
                if let Some(d) = default {
                    self.shadow(d);
                }
                if let Some(n) = namespace {
                    self.shadow(n);
                }
                for (n, alias) in named {
                    let bound = alias.as_ref().unwrap_or(n).clone();
                    self.shadow(&bound);
                }
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.walk_stmt(i);
                }
                if let Some(e) = default_expr {
                    self.walk_expr(*e);
                }
            }
        }
    }

    pub(super) fn walk_expr(&mut self, id: ExprId) {
        let ast = self.ast;
        match ast.get_expr(id) {
            Expr::Ident(_)
            | Expr::String(_)
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::Bool(_)
            | Expr::Uninit
            | Expr::Regex { .. }
            | Expr::Null
            | Expr::Elision
            | Expr::This
            | Expr::NewTarget
            | Expr::Closure { .. } => {}
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left);
                self.walk_expr(*right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::As { expr, .. }
            | Expr::InstanceOf { expr, .. }
            | Expr::Delete { expr } => self.walk_expr(*expr),
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj),
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                self.walk_expr(*callee);
                for a in args {
                    self.walk_expr(*a);
                }
            }
            Expr::Assign { target, value } => {
                self.scan_assign(*target, *value);
                self.walk_expr(*target);
                self.walk_expr(*value);
            }
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                self.walk_expr(*obj);
                self.walk_expr(*index);
            }
            Expr::Array(els) => {
                for e in els {
                    self.walk_expr(*e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    if let Some(k) = ast.objlit_computed_keys.get(e) {
                        self.walk_expr(*k);
                    }
                    self.walk_expr(*e);
                }
            }
            Expr::ArrowFn { params, body, .. } => self.walk_fn_body(params, body),
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.walk_expr(*a);
                }
            }
            Expr::NewDynamic { callee, args } => {
                self.walk_expr(*callee);
                for a in args {
                    self.walk_expr(*a);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond);
                self.walk_expr(*then_branch);
                self.walk_expr(*else_branch);
            }
            Expr::PostIncr { target, .. } => self.walk_expr(*target),
        }
    }

    /// `Stmt::Try` arm body (verbatim, split under the 200-line fn
    /// rule) — each of the three blocks scopes independently, the
    /// catch param shadowing inside the catch scope.
    fn walk_try(
        &mut self,
        body: &[Stmt],
        catch_param: Option<&str>,
        catch_body: &[Stmt],
        finally_body: Option<&[Stmt]>,
    ) {
        self.scopes.push(HashMap::new());
        for s in body {
            self.walk_stmt(s);
        }
        self.scopes.pop();
        self.scopes.push(HashMap::new());
        if let Some(p) = catch_param {
            self.shadow(p);
        }
        for s in catch_body {
            self.walk_stmt(s);
        }
        self.scopes.pop();
        if let Some(fb) = finally_body {
            self.scopes.push(HashMap::new());
            for s in fb {
                self.walk_stmt(s);
            }
            self.scopes.pop();
        }
    }

    /// `Stmt::ClassDecl` arm body (verbatim, same split) — static
    /// field inits / static blocks walk in place, every method body
    /// through the fresh-scope fn walk.
    fn walk_class_decl(
        &mut self,
        static_init: &[StaticInit],
        ctor: Option<&crate::ast::ClassCtor>,
        methods: &[crate::ast::ClassMethod],
        static_methods: &[crate::ast::ClassMethod],
    ) {
        for si in static_init {
            match si {
                StaticInit::Field(f) => self.walk_expr(f.init),
                StaticInit::Block(stmts) => {
                    self.scopes.push(HashMap::new());
                    for s in stmts {
                        self.walk_stmt(s);
                    }
                    self.scopes.pop();
                }
            }
        }
        if let Some(c) = ctor {
            self.walk_fn_body(&c.params, &c.body);
        }
        for m in methods.iter().chain(static_methods.iter()) {
            self.walk_fn_body(&m.params, &m.body);
        }
    }

    /// Fn bodies start from a fresh scope stack: an assign to a free
    /// name inside (a closure-captured rebind of an outer `let`)
    /// scans against nothing and keeps the loud reject — the
    /// module-doc boundary.
    pub(super) fn walk_fn_body(&mut self, params: &[Param], body: &[Stmt]) {
        let saved = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
        for p in params {
            if let Some(d) = p.default {
                self.walk_expr(d);
            }
            self.shadow(&p.name);
        }
        for s in body {
            self.walk_stmt(s);
        }
        self.scopes = saved;
    }
}
