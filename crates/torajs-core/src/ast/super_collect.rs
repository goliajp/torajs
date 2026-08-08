//! Super-call / super-ctor site collection for class desugar.
//!
//! Chunk 350 — extracted from ast.rs. Two logically-independent pairs:
//!
//!   - `collect_supercall_in_stmt` / `collect_supercall_in_expr`
//!     (V3-18 wedge) — surface `super.<method>(args)` call sites,
//!     which the parser encodes as a Call to a marker ident
//!     `__supercall__<method>`; returns `(call ExprId, method name,
//!     args)`.
//!
//!   - `collect_super_in_stmt` / `collect_super_in_expr`
//!     — surface `Expr::Super { args }` (super-ctor invocation)
//!     sites; returns `(call ExprId, args)`.
//!
//! Both traversals share the same shape: a public `_in_stmt` entry
//! that walks statements + a private `_in_expr` helper for the
//! recursive descent into expressions. Callers live in
//! `ast::desugar_classes_super` (super-method / super-ctor rewrite)
//! and reach these two entries through `use super::*` — hence
//! `pub(super)` on the stmt entries.

use super::{Ast, Expr, ExprId, Stmt};

/// V3-18 wedge — collect `super.<method>(args)` call sites in
/// a stmt list. Encoded by the parser as a Call to a marker
/// ident `__supercall__<m>`. Returns (call ExprId, method
/// name, args). Recursive over nested stmts/exprs.
pub(super) fn collect_supercall_in_stmt(
    ast: &Ast,
    s: &Stmt,
    out: &mut Vec<(ExprId, String, Vec<ExprId>)>,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_supercall_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_supercall_in_expr(ast, *value, out),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                collect_supercall_in_expr(ast, *eid, out);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            collect_supercall_in_expr(ast, *init, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_supercall_in_expr(ast, *cond, out);
            collect_supercall_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_supercall_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_supercall_in_expr(ast, *cond, out);
            collect_supercall_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_supercall_in_stmt(ast, body, out);
            collect_supercall_in_expr(ast, *cond, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_supercall_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_supercall_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_supercall_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    collect_supercall_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_supercall_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_supercall_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_supercall_in_expr(ast, *st, out);
            }
            collect_supercall_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                collect_supercall_in_stmt(ast, st, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                collect_supercall_in_stmt(ast, st, out);
            }
            for st in catch_body {
                collect_supercall_in_stmt(ast, st, out);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    collect_supercall_in_stmt(ast, st, out);
                }
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Labeled { body, .. } => collect_supercall_in_stmt(ast, body, out),
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            collect_supercall_in_expr(ast, *parent, out);
            collect_supercall_in_expr(ast, *sep, out);
            collect_supercall_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            collect_supercall_in_expr(ast, *elem_expr, out);
            collect_supercall_in_stmt(ast, body, out);
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                collect_supercall_in_stmt(ast, inner, out);
            }
        }
    }
}

fn collect_supercall_in_expr(ast: &Ast, eid: ExprId, out: &mut Vec<(ExprId, String, Vec<ExprId>)>) {
    if let Expr::Call { callee, args } = ast.get_expr(eid)
        && let Expr::Ident(name) = ast.get_expr(*callee)
        && let Some(m) = name.strip_prefix("__supercall__")
    {
        let m_owned = m.to_string();
        let args_clone = args.clone();
        for a in &args_clone {
            collect_supercall_in_expr(ast, *a, out);
        }
        out.push((eid, m_owned, args_clone));
        return;
    }
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            collect_supercall_in_expr(ast, *callee, out);
            for a in args {
                collect_supercall_in_expr(ast, *a, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_supercall_in_expr(ast, *left, out);
            collect_supercall_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } => collect_supercall_in_expr(ast, *expr, out),
        Expr::Member { obj, .. } => collect_supercall_in_expr(ast, *obj, out),
        Expr::Assign { target, value } => {
            collect_supercall_in_expr(ast, *target, out);
            collect_supercall_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            collect_supercall_in_expr(ast, *obj, out);
            collect_supercall_in_expr(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_supercall_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_supercall_in_expr(ast, *e, out);
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_supercall_in_stmt(ast, s, out);
            }
        }
        Expr::Closure { .. } => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                collect_supercall_in_expr(ast, *a, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            collect_supercall_in_expr(ast, *callee, out);
            for a in args {
                collect_supercall_in_expr(ast, *a, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_supercall_in_expr(ast, *cond, out);
            collect_supercall_in_expr(ast, *then_branch, out);
            collect_supercall_in_expr(ast, *else_branch, out);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => collect_supercall_in_expr(ast, *expr, out),
        Expr::Sequence { left, right } => {
            collect_supercall_in_expr(ast, *left, out);
            collect_supercall_in_expr(ast, *right, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_supercall_in_expr(ast, *lhs, out);
            collect_supercall_in_expr(ast, *rhs, out);
        }
        Expr::OptChain { obj, .. } => collect_supercall_in_expr(ast, *obj, out),
        Expr::OptIndex { obj, index } => {
            collect_supercall_in_expr(ast, *obj, out);
            collect_supercall_in_expr(ast, *index, out);
        }
        Expr::OptCall { callee, args } => {
            collect_supercall_in_expr(ast, *callee, out);
            for a in args {
                collect_supercall_in_expr(ast, *a, out);
            }
        }
        Expr::PostIncr { target, .. } => collect_supercall_in_expr(ast, *target, out),
        _ => {}
    }
}

/// Walk a stmt list and collect every `Expr::Super { args }` site, with
/// the original args slice preserved so the caller can build the
/// rewritten Call. Walks into nested blocks / control flow.
pub(super) fn collect_super_in_stmt(ast: &Ast, s: &Stmt, out: &mut Vec<(ExprId, Vec<ExprId>)>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            collect_super_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => collect_super_in_expr(ast, *value, out),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                collect_super_in_expr(ast, *eid, out);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            collect_super_in_expr(ast, *init, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_super_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_super_in_stmt(ast, body, out);
            collect_super_in_expr(ast, *cond, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            collect_super_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_super_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_super_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    collect_super_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_super_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_super_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_super_in_expr(ast, *st, out);
            }
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                collect_super_in_stmt(ast, st, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                collect_super_in_stmt(ast, st, out);
            }
            for st in catch_body {
                collect_super_in_stmt(ast, st, out);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    collect_super_in_stmt(ast, st, out);
                }
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Labeled { body, .. } => collect_super_in_stmt(ast, body, out),
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            collect_super_in_expr(ast, *parent, out);
            collect_super_in_expr(ast, *sep, out);
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            collect_super_in_expr(ast, *elem_expr, out);
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                collect_super_in_stmt(ast, inner, out);
            }
        }
    }
}

fn collect_super_in_expr(ast: &Ast, eid: ExprId, out: &mut Vec<(ExprId, Vec<ExprId>)>) {
    match ast.get_expr(eid) {
        Expr::Elision => {}
        Expr::Super { args } => {
            // Record the site, then descend into args (super itself
            // probably doesn't nest super(args), but be safe).
            let args_clone = args.clone();
            for a in &args_clone {
                collect_super_in_expr(ast, *a, out);
            }
            out.push((eid, args_clone));
        }
        Expr::Call { callee, args } => {
            collect_super_in_expr(ast, *callee, out);
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_super_in_expr(ast, *left, out);
            collect_super_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } => collect_super_in_expr(ast, *expr, out),
        Expr::Member { obj, .. } => collect_super_in_expr(ast, *obj, out),
        Expr::Assign { target, value } => {
            collect_super_in_expr(ast, *target, out);
            collect_super_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            collect_super_in_expr(ast, *obj, out);
            collect_super_in_expr(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_super_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_super_in_expr(ast, *e, out);
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_super_in_stmt(ast, s, out);
            }
        }
        Expr::Closure { .. } => {}
        Expr::NewDynamic { callee, args } => {
            collect_super_in_expr(ast, *callee, out);
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_expr(ast, *then_branch, out);
            collect_super_in_expr(ast, *else_branch, out);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => collect_super_in_expr(ast, *expr, out),
        Expr::Sequence { left, right } => {
            collect_super_in_expr(ast, *left, out);
            collect_super_in_expr(ast, *right, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_super_in_expr(ast, *lhs, out);
            collect_super_in_expr(ast, *rhs, out);
        }
        Expr::OptChain { obj, .. } => collect_super_in_expr(ast, *obj, out),
        Expr::OptIndex { obj, index } => {
            collect_super_in_expr(ast, *obj, out);
            collect_super_in_expr(ast, *index, out);
        }
        Expr::OptCall { callee, args } => {
            collect_super_in_expr(ast, *callee, out);
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::PostIncr { target, .. } => collect_super_in_expr(ast, *target, out),
        Expr::This
        | Expr::NewTarget
        | Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Uninit => {}
    }
}
