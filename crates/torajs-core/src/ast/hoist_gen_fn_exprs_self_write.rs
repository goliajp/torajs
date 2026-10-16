//! §15.5.5 — a generator expression's body writing its own self-name
//! (rotation 635).
//!
//! The self-name is an immutable binding in the function's own env.
//! A write to it is a TypeError in strict code and silently ignored
//! in sloppy code; either way the RHS is evaluated first (§13.15.2
//! PutValue comes after GetValue of rref). For a written closure the
//! checker marks such a write and the assign lane throws at runtime
//! (`self_name_writes`). A generator expression's body cannot take
//! that lane: after the wrapper route the self-name is a factory
//! param the generator prep rewrites onto `this.<name>`, so the same
//! write would lower to a plain field store. This pass therefore
//! rewrites the write sites in the body BEFORE the prep sees them —
//! strict: `(rhs, throw)`; sloppy: `rhs` — which is what the
//! readonly-global lane does with a write to a non-writable global,
//! and it mints the same TypeError.
//!
//! A nested function or arrow that rebinds the name (param, var,
//! let, function) owns the writes inside it; a block that declares
//! the name, and a `for` whose head does, owns the writes inside it.
//! Those are left alone.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::free_vars_hoisted_names::hoist_var_names;
use super::{Ast, Expr, ExprId, Param, StaticInit, Stmt, UnaryOp};

/// One write to the self-name: the writing expression, its target
/// ident, and the RHS (`None` for `x++`).
struct Site {
    eid: ExprId,
    target: ExprId,
    value: Option<ExprId>,
}

/// Rewrite every write to `sn` inside `body`; answers how many.
pub(crate) fn rewrite_self_name_writes(
    ast: &mut Ast,
    body: &[Stmt],
    sn: &str,
    strict: bool,
) -> usize {
    let mut sites: Vec<Site> = Vec::new();
    if !list_declares(body, sn) {
        for s in body {
            collect_stmt(ast, s, sn, &mut sites);
        }
    }
    apply_sites(ast, &sites, strict);
    sites.len()
}

/// Rewrite every write to the `const` `name` that sits INSIDE a
/// function of `stmts` — a declaration's body, a function
/// expression's, an arrow's — and leave writes at `stmts`' own level
/// alone. The const-write pass uses it: a `const` written from a
/// nested function is a runtime TypeError (§9.1.1.1.5
/// SetMutableBinding on an immutable binding, strict or sloppy),
/// while the same write at the declaring level stays the checker's.
/// Answers how many.
///
/// The binding is read before the throw: SetMutableBinding answers
/// a binding still in its dead zone with a ReferenceError (step 3)
/// before it reaches the immutable check, and `c++` takes GetValue
/// and ToNumeric of the binding before PutValue (§13.4.2.1).
pub(super) fn rewrite_writes_inside_functions(ast: &mut Ast, stmts: &[Stmt], name: &str) -> usize {
    // `stmts` is the list that DECLARES the name (the premise, not
    // a shadow), so no same-level declaration check here.
    let mut sites: Vec<Site> = Vec::new();
    for s in stmts {
        collect_stmt_fns_only(ast, s, name, &mut sites);
    }
    apply_const_sites(ast, &sites, name);
    sites.len()
}

/// [`rewrite_writes_inside_functions`] for a `for` statement whose
/// head declares the `const` `name`: the head's scope covers the
/// test, the step and the body (§14.7.4.2).
pub(super) fn rewrite_writes_inside_functions_of_for(ast: &mut Ast, s: &Stmt, name: &str) -> usize {
    let mut sites: Vec<Site> = Vec::new();
    for e in stmt_exprs(s) {
        collect_expr_fns_only(ast, e, name, &mut sites);
    }
    for c in stmt_children_ref(s) {
        collect_stmt_fns_only(ast, c, name, &mut sites);
    }
    apply_const_sites(ast, &sites, name);
    sites.len()
}

fn apply_const_sites(ast: &mut Ast, sites: &[Site], name: &str) {
    for site in sites {
        let read = match site.value {
            Some(rhs) => {
                let ident = ast.add_expr(Expr::Ident(name.to_string()));
                ast.add_expr(Expr::Sequence {
                    left: rhs,
                    right: ident,
                })
            }
            None => ast.add_expr(Expr::Unary {
                op: UnaryOp::Plus,
                expr: site.target,
            }),
        };
        let right = super::readonly_globals::mint_readonly_throw(ast);
        ast.exprs[site.eid.0 as usize] = Expr::Sequence { left: read, right };
    }
}

fn apply_sites(ast: &mut Ast, sites: &[Site], strict: bool) {
    for site in sites {
        let i = site.eid.0 as usize;
        ast.exprs[i] = match (strict, site.value) {
            (true, Some(rhs)) => {
                let right = super::readonly_globals::mint_readonly_throw(ast);
                Expr::Sequence { left: rhs, right }
            }
            (true, None) => {
                let t = super::readonly_globals::mint_readonly_throw(ast);
                ast.exprs[t.0 as usize].clone()
            }
            (false, Some(rhs)) => ast.exprs[rhs.0 as usize].clone(),
            // `x++` on the function: ToNumeric of the cell, no store.
            (false, None) => Expr::Unary {
                op: UnaryOp::Plus,
                expr: site.target,
            },
        };
    }
}

/// `collect_stmt`'s scope rules, but a write only counts once the
/// walk is inside a function: the same-level shadow arms apply, and
/// every function boundary hands over to the full collector.
fn collect_stmt_fns_only(ast: &Ast, s: &Stmt, sn: &str, out: &mut Vec<Site>) {
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

fn collect_expr_fns_only(ast: &Ast, eid: ExprId, sn: &str, out: &mut Vec<Site>) {
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
fn list_declares(list: &[Stmt], sn: &str) -> bool {
    let mut vars: Vec<String> = Vec::new();
    hoist_var_names(list, &mut vars);
    vars.iter().any(|v| v == sn)
        || super::stmts_flat(list).into_iter().any(|s| {
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

fn collect_stmt(ast: &Ast, s: &Stmt, sn: &str, out: &mut Vec<Site>) {
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
