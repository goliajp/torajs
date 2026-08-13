//! AST fn-body deep clone (RFC 20260804-method-rebind-generic-body
//! blade 1).
//!
//! Clones a `Vec<Stmt>` body so a second FnDecl can carry the same
//! source semantics under fresh ExprIds. Sharing the original ids is
//! not an option: every checker / desugar side-table keys on ExprId,
//! so two functions reading one id would collide in the per-expr
//! type environment (the reason the generic `__cmany_` twin cannot
//! just point at the mono body).
//!
//! Every cloned Expr keeps its source span (diagnostics point at the
//! same user code) and records an `old → new` pair; the consumer
//! hands the pairs to [`super::clone_body_tables::migrate`] so
//! parser-stage side-table entries follow the clone.

use std::collections::HashMap;

use super::ast_def::Ast;
use super::expr::{Expr, Param};
use super::stmt::{ClassCtor, ClassMethod, StaticField, StaticInit, Stmt, SwitchCase};
use crate::ast::ExprId;

pub(crate) struct BodyCloner<'a> {
    pub(crate) ast: &'a mut Ast,
    /// old → new for every cloned Expr node, in clone order.
    pub(crate) map: HashMap<ExprId, ExprId>,
}

impl<'a> BodyCloner<'a> {
    pub(crate) fn new(ast: &'a mut Ast) -> Self {
        Self {
            ast,
            map: HashMap::new(),
        }
    }

    pub(crate) fn clone_stmts(&mut self, body: &[Stmt]) -> Vec<Stmt> {
        body.iter().map(|s| self.clone_stmt(s)).collect()
    }

    pub(crate) fn clone_expr(&mut self, old: ExprId) -> ExprId {
        let e = self.ast.get_expr(old).clone();
        let cloned = match e {
            // Leaves — no child ExprId to rewrite.
            Expr::Ident(_)
            | Expr::String(_)
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::Bool(_)
            | Expr::Uninit
            | Expr::Regex { .. }
            | Expr::Null
            | Expr::This
            | Expr::NewTarget
            | Expr::Elision
            | Expr::Closure { .. } => e,
            Expr::BinOp { op, left, right } => Expr::BinOp {
                op,
                left: self.clone_expr(left),
                right: self.clone_expr(right),
            },
            Expr::Unary { op, expr } => Expr::Unary {
                op,
                expr: self.clone_expr(expr),
            },
            Expr::Member { obj, name } => Expr::Member {
                obj: self.clone_expr(obj),
                name,
            },
            Expr::Call { callee, args } => Expr::Call {
                callee: self.clone_expr(callee),
                args: self.clone_exprs(args),
            },
            Expr::Assign { target, value } => Expr::Assign {
                target: self.clone_expr(target),
                value: self.clone_expr(value),
            },
            Expr::Index { obj, index } => Expr::Index {
                obj: self.clone_expr(obj),
                index: self.clone_expr(index),
            },
            Expr::Array(elems) => Expr::Array(self.clone_exprs(elems)),
            Expr::ObjectLit { fields } => Expr::ObjectLit {
                fields: fields
                    .into_iter()
                    .map(|(n, v)| (n, self.clone_expr(v)))
                    .collect(),
            },
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => Expr::ArrowFn {
                params: self.clone_params(params),
                return_type,
                body: self.clone_stmts(&body),
            },
            Expr::New {
                class_name,
                args,
                type_args,
            } => Expr::New {
                class_name,
                args: self.clone_exprs(args),
                type_args,
            },
            Expr::NewDynamic { callee, args } => Expr::NewDynamic {
                callee: self.clone_expr(callee),
                args: self.clone_exprs(args),
            },
            Expr::Super { args } => Expr::Super {
                args: self.clone_exprs(args),
            },
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => Expr::Ternary {
                cond: self.clone_expr(cond),
                then_branch: self.clone_expr(then_branch),
                else_branch: self.clone_expr(else_branch),
            },
            Expr::Sequence { left, right } => Expr::Sequence {
                left: self.clone_expr(left),
                right: self.clone_expr(right),
            },
            Expr::As { expr, ty_ann } => Expr::As {
                expr: self.clone_expr(expr),
                ty_ann,
            },
            Expr::TypeOf { expr } => Expr::TypeOf {
                expr: self.clone_expr(expr),
            },
            Expr::Delete { expr } => Expr::Delete {
                expr: self.clone_expr(expr),
            },
            Expr::InstanceOf { expr, rhs } => Expr::InstanceOf {
                expr: self.clone_expr(expr),
                rhs: self.clone_expr(rhs),
            },
            Expr::Spread { expr } => Expr::Spread {
                expr: self.clone_expr(expr),
            },
            Expr::Nullish { lhs, rhs } => Expr::Nullish {
                lhs: self.clone_expr(lhs),
                rhs: self.clone_expr(rhs),
            },
            Expr::OptChain { obj, name } => Expr::OptChain {
                obj: self.clone_expr(obj),
                name,
            },
            Expr::OptIndex { obj, index } => Expr::OptIndex {
                obj: self.clone_expr(obj),
                index: self.clone_expr(index),
            },
            Expr::OptCall { callee, args } => Expr::OptCall {
                callee: self.clone_expr(callee),
                args: self.clone_exprs(args),
            },
            Expr::PostIncr { target, is_inc } => Expr::PostIncr {
                target: self.clone_expr(target),
                is_inc,
            },
        };
        let new = self.ast.add_expr(cloned);
        let span = self.ast.expr_spans[old.0 as usize];
        self.ast.set_expr_span(new, span);
        self.map.insert(old, new);
        new
    }

    fn clone_exprs(&mut self, ids: Vec<ExprId>) -> Vec<ExprId> {
        ids.into_iter().map(|id| self.clone_expr(id)).collect()
    }

    fn clone_params(&mut self, params: Vec<Param>) -> Vec<Param> {
        params
            .into_iter()
            .map(|p| Param {
                default: p.default.map(|d| self.clone_expr(d)),
                ..p
            })
            .collect()
    }

    fn clone_stmt(&mut self, s: &Stmt) -> Stmt {
        match s {
            Stmt::Expr(e) => Stmt::Expr(self.clone_expr(*e)),
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            } => Stmt::LetDecl {
                mutable: *mutable,
                name: name.clone(),
                type_ann: type_ann.clone(),
                init: self.clone_expr(*init),
                is_var: *is_var,
            },
            Stmt::UsingDecl {
                name,
                type_ann,
                init,
                is_await,
            } => Stmt::UsingDecl {
                name: name.clone(),
                type_ann: type_ann.clone(),
                init: self.clone_expr(*init),
                is_await: *is_await,
            },
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => Stmt::If {
                cond: self.clone_expr(*cond),
                then_branch: Box::new(self.clone_stmt(then_branch)),
                else_branch: else_branch.as_ref().map(|e| Box::new(self.clone_stmt(e))),
            },
            Stmt::While { cond, body } => Stmt::While {
                cond: self.clone_expr(*cond),
                body: Box::new(self.clone_stmt(body)),
            },
            Stmt::DoWhile { body, cond } => Stmt::DoWhile {
                body: Box::new(self.clone_stmt(body)),
                cond: self.clone_expr(*cond),
            },
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => Stmt::Switch {
                scrutinee: self.clone_expr(*scrutinee),
                cases: cases
                    .iter()
                    .map(|c| SwitchCase {
                        value: self.clone_expr(c.value),
                        body: self.clone_stmts(&c.body),
                    })
                    .collect(),
                default: default.as_ref().map(|d| self.clone_stmts(d)),
            },
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => Stmt::For {
                init: init.as_ref().map(|i| Box::new(self.clone_stmt(i))),
                cond: cond.map(|c| self.clone_expr(c)),
                step: step.map(|st| self.clone_expr(st)),
                body: Box::new(self.clone_stmt(body)),
            },
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => Stmt::ForOfSplitIter {
                var_name: var_name.clone(),
                parent: self.clone_expr(*parent),
                sep: self.clone_expr(*sep),
                body: Box::new(self.clone_stmt(body)),
            },
            Stmt::ForOf {
                var_name,
                var_type_ann,
                src_ident,
                i_ident,
                elem_expr,
                body,
                forin_obj,
                is_await,
            } => Stmt::ForOf {
                var_name: var_name.clone(),
                var_type_ann: var_type_ann.clone(),
                src_ident: src_ident.clone(),
                i_ident: i_ident.clone(),
                elem_expr: self.clone_expr(*elem_expr),
                body: Box::new(self.clone_stmt(body)),
                forin_obj: forin_obj.map(|o| self.clone_expr(o)),
                is_await: *is_await,
            },
            Stmt::Break(l) => Stmt::Break(l.clone()),
            Stmt::Continue(l) => Stmt::Continue(l.clone()),
            Stmt::Labeled { label, body } => Stmt::Labeled {
                label: label.clone(),
                body: Box::new(self.clone_stmt(body)),
            },
            Stmt::Throw(e) => Stmt::Throw(self.clone_expr(*e)),
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => Stmt::Try {
                body: self.clone_stmts(body),
                had_catch: *had_catch,
                catch_param: catch_param.clone(),
                catch_type: catch_type.clone(),
                catch_body: self.clone_stmts(catch_body),
                finally_body: finally_body.as_ref().map(|f| self.clone_stmts(f)),
            },
            Stmt::Block(b) => Stmt::Block(self.clone_stmts(b)),
            Stmt::Multi(b) => Stmt::Multi(self.clone_stmts(b)),
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator,
                span,
            } => Stmt::FnDecl {
                name: name.clone(),
                type_params: type_params.clone(),
                params: self.clone_params(params.clone()),
                return_type: return_type.clone(),
                body: self.clone_stmts(body),
                is_generator: *is_generator,
                span: *span,
            },
            Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => s.clone(),
            Stmt::ClassDecl {
                name,
                type_params,
                parent,
                is_abstract,
                fields,
                static_init,
                ctor,
                methods,
                static_methods,
            } => Stmt::ClassDecl {
                name: name.clone(),
                type_params: type_params.clone(),
                parent: parent.clone(),
                is_abstract: *is_abstract,
                fields: fields.clone(),
                static_init: static_init
                    .iter()
                    .map(|si| match si {
                        StaticInit::Field(f) => StaticInit::Field(StaticField {
                            name: f.name.clone(),
                            type_ann: f.type_ann.clone(),
                            init: self.clone_expr(f.init),
                        }),
                        StaticInit::Block(b) => StaticInit::Block(self.clone_stmts(b)),
                    })
                    .collect(),
                ctor: ctor.as_ref().map(|c| ClassCtor {
                    params: self.clone_params(c.params.clone()),
                    body: self.clone_stmts(&c.body),
                }),
                methods: methods.iter().map(|m| self.clone_class_method(m)).collect(),
                static_methods: static_methods
                    .iter()
                    .map(|m| self.clone_class_method(m))
                    .collect(),
            },
            Stmt::Return(e) => Stmt::Return(e.map(|e| self.clone_expr(e))),
            Stmt::Yield(e) => Stmt::Yield(self.clone_expr(*e)),
            Stmt::YieldInto {
                var,
                type_ann,
                value,
            } => Stmt::YieldInto {
                var: var.clone(),
                type_ann: type_ann.clone(),
                value: self.clone_expr(*value),
            },
            Stmt::ExportDecl {
                inner,
                named,
                default_expr,
                source,
            } => Stmt::ExportDecl {
                inner: inner.as_ref().map(|i| Box::new(self.clone_stmt(i))),
                named: named.clone(),
                default_expr: default_expr.map(|d| self.clone_expr(d)),
                source: source.clone(),
            },
        }
    }

    fn clone_class_method(&mut self, m: &ClassMethod) -> ClassMethod {
        ClassMethod {
            name: m.name.clone(),
            params: self.clone_params(m.params.clone()),
            return_type: m.return_type.clone(),
            body: self.clone_stmts(&m.body),
            is_abstract: m.is_abstract,
            visibility: m.visibility,
            accessor_kind: m.accessor_kind,
            span: m.span,
        }
    }
}
