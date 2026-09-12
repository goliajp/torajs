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

/// The symbol a Contains walk (§8.5.1 / §8.4.1) looks for. Both share
/// the recursion — pierce arrows, stop at anything that binds its own
/// function environment — and differ only in the leaf.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainsSym {
    /// An IdentifierReference spelled `arguments` (§8.4.1).
    Arguments,
    /// The `new.target` meta-property (§8.5.1 Contains NewTarget).
    NewTarget,
}

/// §16.1.1 / §16.2.1.1 — global code (a Script's StatementList, a
/// Module's ModuleItemList) may not contain `new.target`: the walk
/// stops at every function that binds its own [[NewTarget]] and
/// pierces arrows, which inherit the global code's (rotation 636;
/// test262 `global-code/new.target` and `new.target-arrow`).
pub fn global_code_contains_new_target(ast: &Ast) -> bool {
    ast.stmts
        .iter()
        .any(|s| stmt_contains(ast, s, ContainsSym::NewTarget))
}

/// ContainsArguments of a class-field initializer expression.
pub(super) fn init_contains_arguments(ast: &Ast, eid: ExprId) -> bool {
    expr_contains(ast, eid, ContainsSym::Arguments)
}

/// ContainsArguments of a ClassStaticBlockStatementList. §15.7.1
/// states the same early error twice — once for a field initializer,
/// once for a static block — and one walk answers both: `arguments`
/// spelled as an IdentifierReference is true, arrows are pierced (they
/// inherit the outer binding), and anything that binds its own
/// `arguments` stops it. So `static { (function () { return arguments;
/// }) }` and a nested class's method body are both legal, while
/// `static { ((x = arguments) => 0) }` is not.
pub(super) fn static_block_contains_arguments(ast: &Ast, stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|s| stmt_contains(ast, s, ContainsSym::Arguments))
}

fn params_contain(ast: &Ast, params: &[Param], sym: ContainsSym) -> bool {
    // Arrow-param defaults evaluate in the outer `arguments` scope
    // (§8.4.1 recurses on the full ArrowFunction production).
    params
        .iter()
        .any(|p| p.default.is_some_and(|d| expr_contains(ast, d, sym)))
}

fn expr_contains(ast: &Ast, eid: ExprId, sym: ContainsSym) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => sym == ContainsSym::Arguments && n == "arguments",
        Expr::NewTarget => sym == ContainsSym::NewTarget,
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Elision
        | Expr::This
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
        } => expr_contains(ast, *left, sym) || expr_contains(ast, *right, sym),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::PostIncr { target: expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. } => expr_contains(ast, *expr, sym),
        Expr::InstanceOf { expr, rhs } => {
            expr_contains(ast, *expr, sym) || expr_contains(ast, *rhs, sym)
        }
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            expr_contains(ast, *callee, sym) || args.iter().any(|a| expr_contains(ast, *a, sym))
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_contains(ast, *a, sym))
        }
        Expr::Array(items) => items.iter().any(|a| expr_contains(ast, *a, sym)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, v)| expr_contains(ast, *v, sym)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains(ast, *cond, sym)
                || expr_contains(ast, *then_branch, sym)
                || expr_contains(ast, *else_branch, sym)
        }
        Expr::ArrowFn { params, body, .. } => {
            // fn-expr / generator-expr share this node — those bind
            // their own `arguments`, so the walk stops there.
            // …and so does an object-literal method / accessor
            // (MethodDefinition is function code, §15.4): rotation
            // 636's new.target gate refused `{ get x() { return
            // new.target } }` until it stopped here too.
            if ast.fn_expr_exprs.contains(&eid)
                || ast.gen_fn_exprs.contains_key(&eid)
                || ast.objlit_method_exprs.contains(&eid)
            {
                return false;
            }
            params_contain(ast, params, sym) || body.iter().any(|s| stmt_contains(ast, s, sym))
        }
    }
}

fn stmt_contains(ast: &Ast, s: &Stmt, sym: ContainsSym) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_contains(ast, *e, sym),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => expr_contains(ast, *init, sym),
        Stmt::YieldInto { value, .. } => expr_contains(ast, *value, sym),
        Stmt::Return(e) => e.is_some_and(|e| expr_contains(ast, e, sym)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains(ast, *cond, sym)
                || stmt_contains(ast, then_branch, sym)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_contains(ast, s, sym))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_contains(ast, *cond, sym) || stmt_contains(ast, body, sym)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            expr_contains(ast, *scrutinee, sym)
                || cases.iter().any(|c| {
                    expr_contains(ast, c.value, sym)
                        || c.body.iter().any(|s| stmt_contains(ast, s, sym))
                })
                || default
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains(ast, s, sym)))
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref().is_some_and(|s| stmt_contains(ast, s, sym))
                || cond.is_some_and(|e| expr_contains(ast, e, sym))
                || step.is_some_and(|e| expr_contains(ast, e, sym))
                || stmt_contains(ast, body, sym)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_contains(ast, *parent, sym)
                || expr_contains(ast, *sep, sym)
                || stmt_contains(ast, body, sym)
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            expr_contains(ast, *elem_expr, sym)
                || forin_obj.is_some_and(|e| expr_contains(ast, e, sym))
                || stmt_contains(ast, body, sym)
        }
        Stmt::Labeled { body, .. } => stmt_contains(ast, body, sym),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_contains(ast, s, sym))
                || catch_body.iter().any(|s| stmt_contains(ast, s, sym))
                || finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains(ast, s, sym)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_contains(ast, s, sym)),
        Stmt::ExportDecl { inner, .. } => {
            inner.as_deref().is_some_and(|s| stmt_contains(ast, s, sym))
        }
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
