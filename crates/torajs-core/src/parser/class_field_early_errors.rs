//! §15.7.1 class-field early errors — ContainsArguments over a field
//! initializer (ES §8.4.1: `FieldDefinition : ClassElementName
//! Initializer` — it is a Syntax Error if ContainsArguments of
//! Initializer is true).
//!
//! The walk follows the spec's ContainsArguments recursion: an
//! IdentifierReference spelled `arguments` answers true; arrow
//! functions are pierced (they inherit the outer `arguments` binding,
//! §8.4.1 recurses into ArrowFunction); ordinary function expressions
//! and declarations bind their own `arguments`, so the walk stops at
//! them. In this AST both shapes share `Expr::ArrowFn` — a function
//! expression is the same node registered in `ast.fn_expr_exprs`
//! (RFC 20260717-fnexpr-this-channel) or `ast.gen_fn_exprs`, so the
//! boundary test is side-table membership, not the variant. Nested
//! `class` bodies stop the walk too: their own field initializers
//! re-enter this check when they parse, and MethodDefinition bodies
//! bind their own `arguments`.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// ContainsArguments of a class-field initializer expression.
pub(super) fn init_contains_arguments(ast: &Ast, eid: ExprId) -> bool {
    expr_contains(ast, eid)
}

fn params_contain(ast: &Ast, params: &[Param]) -> bool {
    // Arrow-param defaults evaluate in the outer `arguments` scope
    // (§8.4.1 recurses on the full ArrowFunction production).
    params
        .iter()
        .any(|p| p.default.is_some_and(|d| expr_contains(ast, d)))
}

fn expr_contains(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == "arguments",
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Elision
        | Expr::This
        | Expr::NewTarget
        | Expr::Closure { .. } => false,
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
        | Expr::Assign {
            target: left,
            value: right,
        }
        | Expr::Index {
            obj: left,
            index: right,
        }
        | Expr::OptIndex {
            obj: left,
            index: right,
        } => expr_contains(ast, *left) || expr_contains(ast, *right),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::InstanceOf { expr, .. }
        | Expr::PostIncr { target: expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. } => expr_contains(ast, *expr),
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            expr_contains(ast, *callee) || args.iter().any(|a| expr_contains(ast, *a))
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_contains(ast, *a))
        }
        Expr::Array(items) => items.iter().any(|a| expr_contains(ast, *a)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, v)| expr_contains(ast, *v)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains(ast, *cond)
                || expr_contains(ast, *then_branch)
                || expr_contains(ast, *else_branch)
        }
        Expr::ArrowFn { params, body, .. } => {
            // fn-expr / generator-expr share this node — those bind
            // their own `arguments`, so the walk stops there.
            if ast.fn_expr_exprs.contains(&eid) || ast.gen_fn_exprs.contains_key(&eid) {
                return false;
            }
            params_contain(ast, params) || body.iter().any(|s| stmt_contains(ast, s))
        }
    }
}

fn stmt_contains(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_contains(ast, *e),
        Stmt::LetDecl { init, .. } => expr_contains(ast, *init),
        Stmt::YieldInto { value, .. } => expr_contains(ast, *value),
        Stmt::Return(e) => e.is_some_and(|e| expr_contains(ast, e)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains(ast, *cond)
                || stmt_contains(ast, then_branch)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_contains(ast, s))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_contains(ast, *cond) || stmt_contains(ast, body)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            expr_contains(ast, *scrutinee)
                || cases.iter().any(|c| {
                    expr_contains(ast, c.value) || c.body.iter().any(|s| stmt_contains(ast, s))
                })
                || default
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains(ast, s)))
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref().is_some_and(|s| stmt_contains(ast, s))
                || cond.is_some_and(|e| expr_contains(ast, e))
                || step.is_some_and(|e| expr_contains(ast, e))
                || stmt_contains(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_contains(ast, *parent) || expr_contains(ast, *sep) || stmt_contains(ast, body),
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            expr_contains(ast, *elem_expr)
                || forin_obj.is_some_and(|e| expr_contains(ast, e))
                || stmt_contains(ast, body)
        }
        Stmt::Labeled { body, .. } => stmt_contains(ast, body),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_contains(ast, s))
                || catch_body.iter().any(|s| stmt_contains(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains(ast, s)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_contains(ast, s)),
        Stmt::ExportDecl { inner, .. } => inner.as_deref().is_some_and(|s| stmt_contains(ast, s)),
        // `function` decls and nested classes bind their own
        // `arguments` scope (methods / initializers re-check when
        // they parse); the rest carry no expressions.
        Stmt::FnDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_) => false,
    }
}
