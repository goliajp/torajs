//! Where the writes to a name are, scope by scope: the collectors the
//! self-name and const-write rewrites share.

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::free_vars_hoisted_names::hoist_var_names;
use super::super::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// One write to the self-name: the writing expression, its target
/// ident, and the RHS (`None` for `x++`).
pub(super) struct Site {
    pub(super) eid: ExprId,
    pub(super) target: ExprId,
    pub(super) value: Option<ExprId>,
}

/// `collect_stmt`'s scope rules, but a write only counts once the
/// walk is inside a function: the same-level shadow arms apply, and
/// every function boundary hands over to the full collector.
pub(super) fn collect_stmt_fns_only(ast: &Ast, s: &Stmt, sn: &str, out: &mut Vec<Site>) {
    if let Stmt::FnDecl { .. } = s {
        collect_stmt(ast, s, sn, out);
    } else {
        walk_scoped(
            ast,
            s,
            sn,
            out,
            collect_stmt_fns_only,
            collect_expr_fns_only,
        );
    }
}

pub(super) fn collect_expr_fns_only(ast: &Ast, eid: ExprId, sn: &str, out: &mut Vec<Site>) {
    if matches!(ast.get_expr(eid), Expr::ArrowFn { .. }) {
        collect_expr(ast, eid, sn, out);
        return;
    }
    for c in expr_children(ast, eid) {
        collect_expr_fns_only(ast, c, sn, out);
    }
}

fn is_sn(ast: &Ast, eid: ExprId, sn: &str) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == sn)
}

/// Does this statement list declare `sn` as its own binding (a
/// `var` anywhere in it, or a let / function / class at its top,
/// a `Multi` group counting as the top)?
pub(super) fn list_declares(list: &[Stmt], sn: &str) -> bool {
    let mut vars: Vec<String> = Vec::new();
    hoist_var_names(list, &mut vars);
    vars.iter().any(|v| v == sn)
        || super::super::stmts_flat(list).into_iter().any(|s| {
            matches!(s, Stmt::LetDecl { name, .. } | Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } if name == sn)
        })
}

/// §14.7.4.2 — a `for` head's declaration scopes the test, the step
/// and the body: `for (let i = 0; …; i = i + 1)` steps its own `i`.
fn for_head_declares(init: &Stmt, sn: &str) -> bool {
    match init {
        Stmt::LetDecl { name, .. } => name == sn,
        Stmt::Block(list) | Stmt::Multi(list) => list.iter().any(|s| for_head_declares(s, sn)),
        _ => false,
    }
}

fn rebinds(params: &[Param], body: &[Stmt], sn: &str) -> bool {
    params.iter().any(|p| p.name == sn) || list_declares(body, sn)
}

pub(super) fn collect_stmt(ast: &Ast, s: &Stmt, sn: &str, out: &mut Vec<Site>) {
    if let Stmt::FnDecl { params, body, .. } = s {
        collect_fn_body(ast, params, body, sn, out);
    } else {
        walk_scoped(ast, s, sn, out, collect_stmt, collect_expr);
    }
}

/// A function's body, unless its params or body rebind the name.
fn collect_fn_body(ast: &Ast, params: &[Param], body: &[Stmt], sn: &str, out: &mut Vec<Site>) {
    if !rebinds(params, body, sn) {
        for b in body {
            collect_stmt(ast, b, sn, out);
        }
    }
}

type StmtCollect = fn(&Ast, &Stmt, &str, &mut Vec<Site>);
type ExprCollect = fn(&Ast, ExprId, &str, &mut Vec<Site>);

/// The scope rules both collectors share; `stmt` / `expr` re-enter
/// the caller's collector. A statement list that declares the name —
/// a block, a `try` / `catch` / `finally` list, a switch's clauses
/// taken together (§14.12.2, one scope) — owns the writes inside it,
/// as do a `for` head, a for-of variable and a catch parameter (the
/// last over the catch list only; a destructured parameter reaches
/// here as declarations at the top of that list).
fn walk_scoped(
    ast: &Ast,
    s: &Stmt,
    sn: &str,
    out: &mut Vec<Site>,
    stmt: StmtCollect,
    expr: ExprCollect,
) {
    let list = |l: &[Stmt], out: &mut Vec<Site>| {
        if !list_declares(l, sn) {
            for c in l {
                stmt(ast, c, sn, out);
            }
        }
    };
    match s {
        Stmt::Block(l) | Stmt::Multi(l) => list(l, out),
        // A for-of head names a fresh per-iteration binding.
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } if var_name == sn => {}
        Stmt::For {
            init: Some(init), ..
        } if for_head_declares(init, sn) => {}
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            list(body, out);
            if catch_param.as_deref() != Some(sn) {
                list(catch_body, out);
            }
            if let Some(f) = finally_body {
                list(f, out);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            expr(ast, *scrutinee, sn, out);
            let clauses = cases.iter().map(|c| &c.body).chain(default);
            if clauses.clone().any(|b| list_declares(b, sn)) {
                return;
            }
            for c in cases {
                expr(ast, c.value, sn, out);
            }
            for c in clauses.flatten() {
                stmt(ast, c, sn, out);
            }
        }
        Stmt::ClassDecl { .. } => collect_class(ast, s, sn, out, expr),
        _ => {
            for e in stmt_exprs(s) {
                expr(ast, e, sn, out);
            }
            for c in stmt_children_ref(s) {
                stmt(ast, c, sn, out);
            }
        }
    }
}

/// A class's members are functions (the full collector takes them);
/// the heritage expression is evaluated in the enclosing code. A class
/// named `sn` owns the name over its whole body.
fn collect_class(ast: &Ast, s: &Stmt, sn: &str, out: &mut Vec<Site>, expr: ExprCollect) {
    let Stmt::ClassDecl {
        name,
        parent,
        static_init,
        ctor,
        methods,
        static_methods,
        ..
    } = s
    else {
        return;
    };
    if name == sn {
        return;
    }
    if let Some(p) = parent {
        expr(ast, *p, sn, out);
    }
    if let Some(c) = ctor {
        collect_fn_body(ast, &c.params, &c.body, sn, out);
    }
    for m in methods.iter().chain(static_methods) {
        collect_fn_body(ast, &m.params, &m.body, sn, out);
    }
    for i in static_init {
        match i {
            StaticInit::Field(f) => collect_expr(ast, f.init, sn, out),
            StaticInit::Block(b) => collect_fn_body(ast, &[], b, sn, out),
        }
    }
}

fn collect_expr(ast: &Ast, eid: ExprId, sn: &str, out: &mut Vec<Site>) {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } if is_sn(ast, *target, sn) => out.push(Site {
            eid,
            target: *target,
            value: Some(*value),
        }),
        Expr::PostIncr { target, .. } if is_sn(ast, *target, sn) => out.push(Site {
            eid,
            target: *target,
            value: None,
        }),
        // A named function expression's own name is a binding of
        // its own scope (§15.2.5), so it owns the writes in its body.
        Expr::ArrowFn { params, body, .. } => {
            if ast.fn_expr_self_names.get(&eid).map(String::as_str) != Some(sn) {
                collect_fn_body(ast, params, body, sn, out);
            }
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, sn, out);
    }
}
