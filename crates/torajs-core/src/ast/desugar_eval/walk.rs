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

/// What the flag being marked MEANS, which decides where it flips.
#[derive(Clone, Copy)]
enum Mode {
    /// "Inside a function body" — flips true at every callee scope:
    /// function bodies, arrow bodies, class member bodies, parameter
    /// defaults. The original semantics of [`fn_owned_exprs`].
    FnOwned,
    /// "Inside a class member body, through arrows only" — the
    /// §19.2.1.1 home-object question for the eval super gate (r334
    /// blade 6). Flips true at class member bodies, flips back FALSE
    /// at an ordinary `FnDecl` body (its own `this` cuts the home
    /// chain), and rides through arrows unchanged (they pierce).
    ClassOwned,
}

/// One flag per arena slot: is this expression inside a function body?
/// Slots appended after this ran (parsed eval sources) answer `false`
/// via the bounds-checked read in the caller, which is right — a
/// source's own expressions take the position of the call they replace.
pub(crate) fn fn_owned_exprs(ast: &Ast) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    mark_stmts(ast, &ast.stmts, false, Mode::FnOwned, &mut owned);
    owned
}

/// One flag per arena slot: does this expression sit in a class member
/// body (ctor / method / accessor / folded field init / static member
/// or block), reached without crossing an ordinary function boundary?
/// This is exactly "does a direct eval AT THIS SLOT have a
/// [[HomeObject]] on its this-environment" — the §19.2.1.1 steps 4-6
/// verdict the super gate feeds to `parse_eval_source`.
pub(super) fn class_owned_exprs(ast: &Ast) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    mark_stmts(ast, &ast.stmts, false, Mode::ClassOwned, &mut owned);
    owned
}

/// Per-body twin of [`fn_owned_exprs`]: flag exactly the given stmt
/// list's expression sub-tree. RFC 20260808 knife 2 — the bind
/// collectors resolve a by-name use against the fn that SHADOWS the
/// name (its body's uses belong to the local, not the top-level
/// binding), which needs ownership per fn rather than one global bit.
pub(crate) fn body_owned_exprs(ast: &Ast, body: &[Stmt], owned: &mut [bool]) {
    mark_stmts(ast, body, true, Mode::FnOwned, owned);
}

fn mark_stmts(ast: &Ast, stmts: &[Stmt], in_fn: bool, mode: Mode, owned: &mut [bool]) {
    for s in stmts {
        mark_stmt(ast, s, in_fn, mode, owned);
    }
}

fn mark_params(ast: &Ast, params: &[Param], flag: bool, mode: Mode, owned: &mut [bool]) {
    for p in params {
        if let Some(d) = p.default {
            mark_expr(ast, d, flag, mode, owned);
        }
    }
}

fn mark_stmt(ast: &Ast, s: &Stmt, in_fn: bool, mode: Mode, owned: &mut [bool]) {
    // The flag a callee-scope body is marked with: FnOwned flips true
    // at any function boundary; ClassOwned flips FALSE at an ordinary
    // function body (own `this`, no home) — class members below flip
    // true in both modes.
    let fn_body_flag = matches!(mode, Mode::FnOwned);
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => mark_expr(ast, *e, in_fn, mode, owned),
        Stmt::Return(e) => {
            if let Some(e) = e {
                mark_expr(ast, *e, in_fn, mode, owned);
            }
        }
        Stmt::YieldInto { value, .. } => mark_expr(ast, *value, in_fn, mode, owned),
        Stmt::LetDecl { init, .. } => mark_expr(ast, *init, in_fn, mode, owned),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, in_fn, mode, owned);
            mark_stmt(ast, then_branch, in_fn, mode, owned);
            if let Some(e) = else_branch.as_deref() {
                mark_stmt(ast, e, in_fn, mode, owned);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            mark_expr(ast, *cond, in_fn, mode, owned);
            mark_stmt(ast, body, in_fn, mode, owned);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            mark_expr(ast, *scrutinee, in_fn, mode, owned);
            for c in cases {
                mark_expr(ast, c.value, in_fn, mode, owned);
                mark_stmts(ast, &c.body, in_fn, mode, owned);
            }
            if let Some(d) = default {
                mark_stmts(ast, d, in_fn, mode, owned);
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref() {
                mark_stmt(ast, i, in_fn, mode, owned);
            }
            for e in [cond, step].into_iter().flatten() {
                mark_expr(ast, *e, in_fn, mode, owned);
            }
            mark_stmt(ast, body, in_fn, mode, owned);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            mark_expr(ast, *parent, in_fn, mode, owned);
            mark_expr(ast, *sep, in_fn, mode, owned);
            mark_stmt(ast, body, in_fn, mode, owned);
        }
        Stmt::ForOf {
            elem_expr,
            forin_obj,
            body,
            ..
        } => {
            mark_expr(ast, *elem_expr, in_fn, mode, owned);
            if let Some(o) = forin_obj {
                mark_expr(ast, *o, in_fn, mode, owned);
            }
            mark_stmt(ast, body, in_fn, mode, owned);
        }
        Stmt::Labeled { body, .. } => mark_stmt(ast, body, in_fn, mode, owned),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            mark_stmts(ast, body, in_fn, mode, owned);
            mark_stmts(ast, catch_body, in_fn, mode, owned);
            if let Some(f) = finally_body {
                mark_stmts(ast, f, in_fn, mode, owned);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => mark_stmts(ast, b, in_fn, mode, owned),
        Stmt::FnDecl { params, body, .. } => {
            mark_params(ast, params, fn_body_flag, mode, owned);
            mark_stmts(ast, body, fn_body_flag, mode, owned);
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
                    super::super::StaticInit::Field(f) => mark_expr(ast, f.init, true, mode, owned),
                    super::super::StaticInit::Block(b) => mark_stmts(ast, b, true, mode, owned),
                }
            }
            if let Some(c) = ctor {
                mark_params(ast, &c.params, true, mode, owned);
                mark_stmts(ast, &c.body, true, mode, owned);
            }
            for m in methods.iter().chain(static_methods) {
                mark_params(ast, &m.params, true, mode, owned);
                mark_stmts(ast, &m.body, true, mode, owned);
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner.as_deref() {
                mark_stmt(ast, i, in_fn, mode, owned);
            }
            if let Some(e) = default_expr {
                mark_expr(ast, *e, in_fn, mode, owned);
            }
        }
        _ => {}
    }
}

fn mark_expr(ast: &Ast, eid: ExprId, in_fn: bool, mode: Mode, owned: &mut [bool]) {
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
            mark_expr(ast, *left, in_fn, mode, owned);
            mark_expr(ast, *right, in_fn, mode, owned);
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::Spread { expr } => mark_expr(ast, *expr, in_fn, mode, owned),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            mark_expr(ast, *obj, in_fn, mode, owned)
        }
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            mark_expr(ast, *callee, in_fn, mode, owned);
            for a in args {
                mark_expr(ast, *a, in_fn, mode, owned);
            }
        }
        Expr::Assign { target, value } => {
            mark_expr(ast, *target, in_fn, mode, owned);
            mark_expr(ast, *value, in_fn, mode, owned);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            mark_expr(ast, *obj, in_fn, mode, owned);
            mark_expr(ast, *index, in_fn, mode, owned);
        }
        Expr::Array(elems) => {
            for e in elems {
                mark_expr(ast, *e, in_fn, mode, owned);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                mark_expr(ast, *v, in_fn, mode, owned);
            }
        }
        // An arrow is a callee scope for FnOwned, but transparent for
        // ClassOwned — it pierces `this`/`super` to its enclosure, so
        // its body keeps the enclosure's home verdict.
        Expr::ArrowFn { params, body, .. } => {
            let body_flag = match mode {
                Mode::FnOwned => true,
                Mode::ClassOwned => in_fn,
            };
            mark_params(ast, params, body_flag, mode, owned);
            mark_stmts(ast, body, body_flag, mode, owned);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                mark_expr(ast, *a, in_fn, mode, owned);
            }
        }
        Expr::NewDynamic { callee, args } => {
            mark_expr(ast, *callee, in_fn, mode, owned);
            for a in args {
                mark_expr(ast, *a, in_fn, mode, owned);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_expr(ast, *cond, in_fn, mode, owned);
            mark_expr(ast, *then_branch, in_fn, mode, owned);
            mark_expr(ast, *else_branch, in_fn, mode, owned);
        }
        Expr::PostIncr { target, .. } => mark_expr(ast, *target, in_fn, mode, owned),
        _ => {}
    }
}
