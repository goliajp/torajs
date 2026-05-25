//! `count_refs_stmt` / `count_refs_expr` — mutually-recursive AST
//! walkers that tally every `Ident` reference per name. Used by the
//! `unused-let` / `unused-import` lint rules to find declarations
//! with zero downstream readers.
//!
//! Both fns operate on `&Ast` + immutable Stmt/ExprId arguments +
//! `&mut HashMap<String, usize>`; no other state.
//!
//! Extracted from `linter.rs` (2026-05-25, god-file decomp).

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, StaticInit, Stmt};

pub(super) fn count_refs_stmt(ast: &Ast, s: &Stmt, refs: &mut HashMap<String, usize>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => count_refs_expr(ast, *eid, refs),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                count_refs_expr(ast, *e, refs);
            }
        }
        Stmt::LetDecl { init, .. } => count_refs_expr(ast, *init, refs),
        Stmt::YieldInto { value, .. } => count_refs_expr(ast, *value, refs),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_stmt(ast, then_branch, refs);
            if let Some(eb) = else_branch {
                count_refs_stmt(ast, eb, refs);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                count_refs_stmt(ast, i, refs);
            }
            if let Some(c) = cond {
                count_refs_expr(ast, *c, refs);
            }
            if let Some(st) = step {
                count_refs_expr(ast, *st, refs);
            }
            count_refs_stmt(ast, body, refs);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            count_refs_expr(ast, *parent, refs);
            count_refs_expr(ast, *sep, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            count_refs_expr(ast, *elem_expr, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            count_refs_expr(ast, *scrutinee, refs);
            for c in cases {
                count_refs_expr(ast, c.value, refs);
                for s in &c.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            if let Some(d) = default {
                for s in d {
                    count_refs_stmt(ast, s, refs);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
            for s in catch_body {
                count_refs_stmt(ast, s, refs);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    count_refs_stmt(ast, s, refs);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                count_refs_stmt(ast, s, refs);
            }
        }
        Stmt::FnDecl { body, params, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    count_refs_expr(ast, d, refs);
                }
            }
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                for s in &c.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for s in &m.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            for si in static_init {
                match si {
                    StaticInit::Field(sf) => count_refs_expr(ast, sf.init, refs),
                    StaticInit::Block(stmts) => {
                        for s in stmts {
                            count_refs_stmt(ast, s, refs);
                        }
                    }
                }
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(s) = inner {
                count_refs_stmt(ast, s, refs);
            }
            if let Some(e) = default_expr {
                count_refs_expr(ast, *e, refs);
            }
        }
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } | Stmt::Break | Stmt::Continue => {}
    }
}

pub(super) fn count_refs_expr(ast: &Ast, eid: ExprId, refs: &mut HashMap<String, usize>) {
    match ast.get_expr(eid) {
        Expr::Ident(name) => {
            *refs.entry(name.clone()).or_insert(0) += 1;
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => count_refs_expr(ast, *obj, refs),
        Expr::Index { obj, index } => {
            count_refs_expr(ast, *obj, refs);
            count_refs_expr(ast, *index, refs);
        }
        Expr::Call { callee, args } => {
            count_refs_expr(ast, *callee, refs);
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Assign { target, value } => {
            count_refs_expr(ast, *target, refs);
            count_refs_expr(ast, *value, refs);
        }
        Expr::Array(items) => {
            for e in items {
                count_refs_expr(ast, *e, refs);
            }
        }
        Expr::Spread { expr } => count_refs_expr(ast, *expr, refs),
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                count_refs_expr(ast, *v, refs);
            }
        }
        Expr::ArrowFn { body, params, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    count_refs_expr(ast, d, refs);
                }
            }
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
        }
        Expr::Closure { fn_name, captures } => {
            *refs.entry(fn_name.clone()).or_insert(0) += 1;
            for c in captures {
                *refs.entry(c.clone()).or_insert(0) += 1;
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Super { args } => {
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_expr(ast, *then_branch, refs);
            count_refs_expr(ast, *else_branch, refs);
        }
        Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            count_refs_expr(ast, *expr, refs)
        }
        Expr::InstanceOf { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::As { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::Sequence { left, right } => {
            count_refs_expr(ast, *left, refs);
            count_refs_expr(ast, *right, refs);
        }
        Expr::Nullish { lhs, rhs } => {
            count_refs_expr(ast, *lhs, refs);
            count_refs_expr(ast, *rhs, refs);
        }
        Expr::Unary { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::BinOp { left, right, .. } => {
            count_refs_expr(ast, *left, refs);
            count_refs_expr(ast, *right, refs);
        }
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget => {}
    }
}
