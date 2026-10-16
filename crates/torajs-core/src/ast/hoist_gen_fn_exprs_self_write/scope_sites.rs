//! Where the writes to a name are, scope by scope: the collectors the
//! self-name and const-write rewrites share.

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::free_vars_hoisted_names::hoist_var_names;
use super::super::{Ast, ClassCtor, Expr, ExprId, Param, StaticInit, Stmt};
use std::ops::Range;

/// One write to the self-name: the writing expression, its target
/// ident, the RHS (`None` for `x++`), and whether it sits in a class
/// body, which is strict code whatever the enclosing goal (§11.2.2).
pub(super) struct Site {
    pub(super) eid: ExprId,
    pub(super) target: ExprId,
    pub(super) value: Option<ExprId>,
    pub(super) in_class: bool,
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

/// A function's parameter defaults and body, unless its params or
/// body rebind the name. A default is code of the function
/// (§10.2.11); `split_param_scope` has already renamed a body
/// declaration a default would reach.
fn collect_fn_body(ast: &Ast, params: &[Param], body: &[Stmt], sn: &str, out: &mut Vec<Site>) {
    collect_fn_body_except(ast, params, body, 0..0, sn, out);
}

/// [`collect_fn_body`] skipping the statements at `skip`.
fn collect_fn_body_except(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    skip: Range<usize>,
    sn: &str,
    out: &mut Vec<Site>,
) {
    if rebinds(params, body, sn) {
        return;
    }
    for d in params.iter().filter_map(|p| p.default) {
        collect_expr(ast, d, sn, out);
    }
    for (i, b) in body.iter().enumerate() {
        if !skip.contains(&i) {
            collect_stmt(ast, b, sn, out);
        }
    }
}

/// A constructor. The instance field initializers the parser spliced
/// into its body run in the class body's scope (§15.7.10), where the
/// constructor's parameters and locals do not reach, so they are
/// collected apart from it.
fn collect_ctor(ast: &Ast, class: &str, c: &ClassCtor, sn: &str, out: &mut Vec<Site>) {
    let fields = ctor_field_inits(ast, class, &c.body);
    for s in &c.body[fields.clone()] {
        collect_stmt(ast, s, sn, out);
    }
    collect_fn_body_except(ast, &c.params, &c.body, fields, sn, out);
}

/// The span of a constructor body the parser filled with the class's
/// instance field initializers: from just past the `super(...)`
/// statement (or the top) to the end `ctor_field_init_end` records,
/// or the whole body of a constructor synthesized only to hold them.
/// Empty when that span does not read back as initializers.
pub(in crate::ast) fn ctor_field_inits(ast: &Ast, class: &str, body: &[Stmt]) -> Range<usize> {
    let start = body
        .iter()
        .position(|s| matches!(s, Stmt::Expr(e) if matches!(ast.get_expr(*e), Expr::Super { .. })))
        .map_or(0, |i| i + 1);
    let end = if ast.field_init_synth_ctors.contains(class) {
        body.len()
    } else {
        match ast.ctor_field_init_end.get(class) {
            Some(&e) => e,
            None => return 0..0,
        }
    };
    if start > end || end > body.len() || !body[start..end].iter().all(|s| is_field_init(ast, s)) {
        return 0..0;
    }
    start..end
}

/// `this.<f> = init` (also keyed), a private field's install marker,
/// or a class the initializer declared in front of itself.
fn is_field_init(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::Expr(e) = s else {
        return matches!(s, Stmt::ClassDecl { .. });
    };
    if super::super::private_brand::is_field_add(ast, *e) {
        return true;
    }
    let Expr::Assign { target, .. } = ast.get_expr(*e) else {
        return false;
    };
    let (Expr::Member { obj, .. } | Expr::Index { obj, .. }) = ast.get_expr(*target) else {
        return false;
    };
    match ast.get_expr(*obj) {
        Expr::This => true,
        Expr::As { expr, .. } => matches!(ast.get_expr(*expr), Expr::This),
        _ => false,
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
        // A `Multi` group shares the enclosing list's scope, which has
        // already asked whether it declares the name.
        Stmt::Block(l) => list(l, out),
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
/// named `sn` owns the name over its whole body. Every write found
/// here is in strict code (§11.2.2).
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
    let mark = out.len();
    if let Some(p) = parent {
        expr(ast, *p, sn, out);
    }
    if let Some(c) = ctor {
        collect_ctor(ast, name, c, sn, out);
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
    for site in &mut out[mark..] {
        site.in_class = true;
    }
}

fn collect_expr(ast: &Ast, eid: ExprId, sn: &str, out: &mut Vec<Site>) {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } if is_sn(ast, *target, sn) => out.push(Site {
            eid,
            target: *target,
            value: Some(*value),
            in_class: false,
        }),
        Expr::PostIncr { target, .. } if is_sn(ast, *target, sn) => out.push(Site {
            eid,
            target: *target,
            value: None,
            in_class: false,
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
