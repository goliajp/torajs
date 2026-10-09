//! The statement and expression walk of the scoped type declaration
//! hoist: every annotation a scope holds, through every compound
//! statement and every expression that carries one.

use super::{Env, Hoist};
use crate::ast::{Ast, Expr, ExprId, StaticInit, Stmt};

impl Hoist {
    pub(super) fn stmt(&mut self, ast: &mut Ast, s: &mut Stmt, env: &Env) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => self.expr(ast, *e, env),
            Stmt::Return(v) => {
                if let Some(e) = v {
                    self.expr(ast, *e, env);
                }
            }
            Stmt::LetDecl { type_ann, init, .. } | Stmt::UsingDecl { type_ann, init, .. } => {
                env.sub_opt(type_ann);
                self.expr(ast, *init, env);
            }
            Stmt::YieldInto {
                type_ann, value, ..
            } => {
                env.sub_opt(type_ann);
                self.expr(ast, *value, env);
            }
            Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::ForOfSplitIter { .. }
            | Stmt::ForOf { .. }
            | Stmt::Labeled { .. } => self.loop_or_branch(ast, s, env),
            Stmt::Switch { .. } | Stmt::Try { .. } => self.clause_scopes(ast, s, env),
            Stmt::Block(v) => self.scope(ast, v, env),
            Stmt::Multi(v) => self.list(ast, v, env),
            Stmt::FnDecl {
                type_params,
                params,
                return_type,
                body,
                ..
            } => self.function(ast, type_params, params, return_type, body, env),
            Stmt::TypeDecl {
                type_params,
                fields,
                ..
            } => {
                let env = env.under_tparams(type_params);
                for (_, ty) in fields.iter_mut() {
                    env.sub(ty);
                }
            }
            Stmt::ClassDecl { .. } => self.class(ast, s, env),
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.stmt(ast, i, env);
                }
                if let Some(e) = default_expr {
                    self.expr(ast, *e, env);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::ImportDecl { .. } => {}
        }
    }

    /// A branch or a loop: its body is a statement, a scope only when
    /// it is a block.
    fn loop_or_branch(&mut self, ast: &mut Ast, s: &mut Stmt, env: &Env) {
        match s {
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(ast, *cond, env);
                self.stmt(ast, then_branch, env);
                if let Some(b) = else_branch {
                    self.stmt(ast, b, env);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
                self.expr(ast, *cond, env);
                self.stmt(ast, body, env);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(ast, i, env);
                }
                for e in cond.iter().chain(step.iter()) {
                    self.expr(ast, *e, env);
                }
                self.stmt(ast, body, env);
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.expr(ast, *parent, env);
                self.expr(ast, *sep, env);
                self.stmt(ast, body, env);
            }
            Stmt::ForOf {
                var_type_ann,
                elem_expr,
                body,
                forin_obj,
                ..
            } => {
                env.sub_opt(var_type_ann);
                self.expr(ast, *elem_expr, env);
                if let Some(o) = forin_obj {
                    self.expr(ast, *o, env);
                }
                self.stmt(ast, body, env);
            }
            Stmt::Labeled { body, .. } => self.stmt(ast, body, env),
            _ => {}
        }
    }

    /// A switch or a try: statement lists that are scopes of their own.
    fn clause_scopes(&mut self, ast: &mut Ast, s: &mut Stmt, env: &Env) {
        match s {
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(ast, *scrutinee, env);
                // one block: a declaration in any clause is visible in all
                let mut lists: Vec<&mut Vec<Stmt>> =
                    cases.iter_mut().map(|c| &mut c.body).collect();
                lists.extend(default.iter_mut());
                let env = self.declare(ast, &mut lists, env);
                for c in cases.iter_mut() {
                    self.expr(ast, c.value, &env);
                    self.list(ast, &mut c.body, &env);
                }
                if let Some(d) = default {
                    self.list(ast, d, &env);
                }
            }
            Stmt::Try {
                body,
                catch_type,
                catch_body,
                finally_body,
                ..
            } => {
                env.sub_opt(catch_type);
                self.scope(ast, body, env);
                self.scope(ast, catch_body, env);
                if let Some(f) = finally_body {
                    self.scope(ast, f, env);
                }
            }
            _ => {}
        }
    }

    fn class(&mut self, ast: &mut Ast, s: &mut Stmt, env: &Env) {
        let Stmt::ClassDecl {
            type_params,
            parent,
            fields,
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } = s
        else {
            return;
        };
        let env = env.under_tparams(type_params);
        if let Some(p) = parent {
            self.expr(ast, *p, &env);
        }
        for (_, ty) in fields.iter_mut() {
            env.sub(ty);
        }
        for si in static_init.iter_mut() {
            match si {
                StaticInit::Field(f) => {
                    env.sub(&mut f.type_ann);
                    self.expr(ast, f.init, &env);
                }
                StaticInit::Block(v) => self.scope(ast, v, &env),
            }
        }
        if let Some(c) = ctor {
            self.function(ast, &[], &mut c.params, &mut None, &mut c.body, &env);
        }
        for m in methods.iter_mut().chain(static_methods.iter_mut()) {
            self.function(
                ast,
                &m.type_params,
                &mut m.params,
                &mut m.return_type,
                &mut m.body,
                &env,
            );
        }
    }

    pub(super) fn expr(&mut self, ast: &mut Ast, eid: ExprId, env: &Env) {
        if let Some(args) = ast.call_type_args.get_mut(&eid) {
            for a in args.iter_mut() {
                env.sub(a);
            }
        }
        match &mut ast.exprs[eid.0 as usize] {
            Expr::As { ty_ann, .. } => env.sub(ty_ann),
            Expr::New { type_args, .. } => {
                for t in type_args.iter_mut() {
                    env.sub(t);
                }
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                let mut params = std::mem::take(params);
                let mut ret = return_type.take();
                let mut body = std::mem::take(body);
                self.function(ast, &[], &mut params, &mut ret, &mut body, env);
                if let Expr::ArrowFn {
                    params: p,
                    return_type: r,
                    body: b,
                } = &mut ast.exprs[eid.0 as usize]
                {
                    *p = params;
                    *r = ret;
                    *b = body;
                }
                return;
            }
            _ => {}
        }
        let mut kids: Vec<ExprId> = Vec::new();
        crate::ast::desugar_async_sm_walk::visit_expr_children(ast, eid, &mut |c| kids.push(c));
        for k in kids {
            self.expr(ast, k, env);
        }
    }
}
