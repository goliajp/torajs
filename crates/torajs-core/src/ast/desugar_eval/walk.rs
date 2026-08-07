//! Position analysis for the eval desugar: which expression slots sit
//! inside a function body?
//!
//! An INDIRECT eval's source belongs to the global scope. At the top
//! level that scope and the surrounding scope are the same thing
//! (script framing — module doc), so a top-level value-position call
//! can collapse even when its source reads identifiers. Inside a
//! function the two scopes differ — a collapsed source would wrongly
//! see the function's locals — so the value walk needs to know, per
//! arena slot, whether the slot is under a function body. The
//! statement walk threads an `in_fn` flag; the expression arena has no
//! positions, so this walk reconstructs them once up front.
//!
//! Marking MORE slots as fn-owned is the safe direction (a missed
//! collapse is an honest reject; a wrong collapse is silent-wrong), so
//! everything that runs in a callee scope — function bodies, arrow
//! bodies, class ctor/method/static bodies, parameter defaults — marks
//! its whole expression tree.

use super::super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// Every lexical name declared BELOW the top-level statement list —
/// `let` / `const` / `class` inside blocks and control flow, loop
/// bindings, catch parameters. A top-level-position indirect eval
/// whose source reads one of these names cannot collapse: the name
/// would resolve to the enclosing block's binding, while real global
/// code cannot see it (a top-level `var` or `let` CAN be read — the
/// global environment holds both — so only nested lexical names
/// threaten the collapse). Function bodies are walked too; their names
/// over-collect harmlessly, since slots inside them are already
/// declined by `fn_owned_exprs`.
pub(super) fn nested_lexical_names(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_list(&ast.stmts, false, &mut out);
    out
}

fn collect_list(stmts: &[Stmt], nested: bool, out: &mut HashSet<String>) {
    for s in stmts {
        collect_stmt(s, nested, out);
    }
}

fn collect_stmt(s: &Stmt, nested: bool, out: &mut HashSet<String>) {
    match s {
        Stmt::LetDecl { name, is_var, .. } => {
            if nested && !is_var {
                out.insert(name.clone());
            }
        }
        Stmt::ClassDecl { name, .. } => {
            if nested {
                out.insert(name.clone());
            }
        }
        Stmt::FnDecl { body, .. } => collect_list(body, true, out),
        Stmt::Block(b) => collect_list(b, true, out),
        // `Multi` is the parser's same-level expansion of `var x, y` —
        // it introduces no scope of its own.
        Stmt::Multi(b) => collect_list(b, nested, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_stmt(then_branch, true, out);
            if let Some(e) = else_branch.as_deref() {
                collect_stmt(e, true, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_stmt(body, true, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref() {
                collect_stmt(i, true, out);
            }
            collect_stmt(body, true, out);
        }
        Stmt::ForOf { var_name, body, .. } => {
            // The loop binding is lexical and per-iteration — never a
            // global name.
            out.insert(var_name.clone());
            collect_stmt(body, true, out);
        }
        Stmt::ForOfSplitIter { var_name, body, .. } => {
            out.insert(var_name.clone());
            collect_stmt(body, true, out);
        }
        Stmt::Labeled { body, .. } => collect_stmt(body, nested, out),
        Stmt::Try {
            catch_param,
            body,
            catch_body,
            finally_body,
            ..
        } => {
            if let Some(p) = catch_param {
                out.insert(p.clone());
            }
            collect_list(body, true, out);
            collect_list(catch_body, true, out);
            if let Some(f) = finally_body {
                collect_list(f, true, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_list(&c.body, true, out);
            }
            if let Some(d) = default {
                collect_list(d, true, out);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner.as_deref() {
                collect_stmt(i, nested, out);
            }
        }
        _ => {}
    }
}

/// One flag per arena slot: is this expression inside a function body?
/// Slots appended after this ran (parsed eval sources) answer `false`
/// via the bounds-checked read in the caller, which is right — a
/// source's own expressions take the position of the call they replace.
pub(super) fn fn_owned_exprs(ast: &Ast) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    mark_stmts(ast, &ast.stmts, false, &mut owned);
    owned
}

fn mark_stmts(ast: &Ast, stmts: &[Stmt], in_fn: bool, owned: &mut [bool]) {
    for s in stmts {
        mark_stmt(ast, s, in_fn, owned);
    }
}

fn mark_params(ast: &Ast, params: &[Param], owned: &mut [bool]) {
    for p in params {
        if let Some(d) = p.default {
            mark_expr(ast, d, true, owned);
        }
    }
}

fn mark_stmt(ast: &Ast, s: &Stmt, in_fn: bool, owned: &mut [bool]) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => mark_expr(ast, *e, in_fn, owned),
        Stmt::Return(e) => {
            if let Some(e) = e {
                mark_expr(ast, *e, in_fn, owned);
            }
        }
        Stmt::YieldInto { value, .. } => mark_expr(ast, *value, in_fn, owned),
        Stmt::LetDecl { init, .. } => mark_expr(ast, *init, in_fn, owned),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, in_fn, owned);
            mark_stmt(ast, then_branch, in_fn, owned);
            if let Some(e) = else_branch.as_deref() {
                mark_stmt(ast, e, in_fn, owned);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            mark_expr(ast, *cond, in_fn, owned);
            mark_stmt(ast, body, in_fn, owned);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            mark_expr(ast, *scrutinee, in_fn, owned);
            for c in cases {
                mark_expr(ast, c.value, in_fn, owned);
                mark_stmts(ast, &c.body, in_fn, owned);
            }
            if let Some(d) = default {
                mark_stmts(ast, d, in_fn, owned);
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref() {
                mark_stmt(ast, i, in_fn, owned);
            }
            for e in [cond, step].into_iter().flatten() {
                mark_expr(ast, *e, in_fn, owned);
            }
            mark_stmt(ast, body, in_fn, owned);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            mark_expr(ast, *parent, in_fn, owned);
            mark_expr(ast, *sep, in_fn, owned);
            mark_stmt(ast, body, in_fn, owned);
        }
        Stmt::ForOf {
            elem_expr,
            forin_obj,
            body,
            ..
        } => {
            mark_expr(ast, *elem_expr, in_fn, owned);
            if let Some(o) = forin_obj {
                mark_expr(ast, *o, in_fn, owned);
            }
            mark_stmt(ast, body, in_fn, owned);
        }
        Stmt::Labeled { body, .. } => mark_stmt(ast, body, in_fn, owned),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            mark_stmts(ast, body, in_fn, owned);
            mark_stmts(ast, catch_body, in_fn, owned);
            if let Some(f) = finally_body {
                mark_stmts(ast, f, in_fn, owned);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => mark_stmts(ast, b, in_fn, owned),
        Stmt::FnDecl { params, body, .. } => {
            mark_params(ast, params, owned);
            mark_stmts(ast, body, true, owned);
        }
        Stmt::ClassDecl {
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            // Every class member body runs in a callee scope. Static
            // field initializers and static blocks run at class
            // definition time but in the class's own scope, not the
            // top level — conservative direction, mark them.
            for si in static_init {
                match si {
                    super::super::StaticInit::Field(f) => mark_expr(ast, f.init, true, owned),
                    super::super::StaticInit::Block(b) => mark_stmts(ast, b, true, owned),
                }
            }
            if let Some(c) = ctor {
                mark_params(ast, &c.params, owned);
                mark_stmts(ast, &c.body, true, owned);
            }
            for m in methods.iter().chain(static_methods) {
                mark_params(ast, &m.params, owned);
                mark_stmts(ast, &m.body, true, owned);
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner.as_deref() {
                mark_stmt(ast, i, in_fn, owned);
            }
            if let Some(e) = default_expr {
                mark_expr(ast, *e, in_fn, owned);
            }
        }
        _ => {}
    }
}

fn mark_expr(ast: &Ast, eid: ExprId, in_fn: bool, owned: &mut [bool]) {
    let Some(slot) = owned.get_mut(eid.0 as usize) else {
        return;
    };
    if in_fn {
        *slot = true;
    }
    let Some(e) = ast.exprs.get(eid.0 as usize) else {
        return;
    };
    match e {
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            mark_expr(ast, *left, in_fn, owned);
            mark_expr(ast, *right, in_fn, owned);
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::Spread { expr } => mark_expr(ast, *expr, in_fn, owned),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => mark_expr(ast, *obj, in_fn, owned),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            mark_expr(ast, *callee, in_fn, owned);
            for a in args {
                mark_expr(ast, *a, in_fn, owned);
            }
        }
        Expr::Assign { target, value } => {
            mark_expr(ast, *target, in_fn, owned);
            mark_expr(ast, *value, in_fn, owned);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            mark_expr(ast, *obj, in_fn, owned);
            mark_expr(ast, *index, in_fn, owned);
        }
        Expr::Array(elems) => {
            for e in elems {
                mark_expr(ast, *e, in_fn, owned);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                mark_expr(ast, *v, in_fn, owned);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            mark_params(ast, params, owned);
            mark_stmts(ast, body, true, owned);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                mark_expr(ast, *a, in_fn, owned);
            }
        }
        Expr::NewDynamic { callee, args } => {
            mark_expr(ast, *callee, in_fn, owned);
            for a in args {
                mark_expr(ast, *a, in_fn, owned);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, in_fn, owned);
            mark_expr(ast, *then_branch, in_fn, owned);
            mark_expr(ast, *else_branch, in_fn, owned);
        }
        Expr::PostIncr { target, .. } => mark_expr(ast, *target, in_fn, owned),
        _ => {}
    }
}
