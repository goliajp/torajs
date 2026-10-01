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
use super::{Ast, Expr, ExprId, Param, Stmt, UnaryOp};

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

/// Rewrite every write to `name` that sits INSIDE a function of
/// `stmts` — a declaration's body, a function expression's, an
/// arrow's — and leave writes at `stmts`' own level alone. The
/// const-write pass uses it (rotation 636): a top-level `const`
/// written from a nested function is a runtime TypeError (§9.1.1.1.5
/// SetMutableBinding on an immutable binding, strict or sloppy),
/// while the same write at the top level stays the checker's
/// compile-time refusal. Answers how many.
pub(super) fn rewrite_writes_inside_functions(ast: &mut Ast, stmts: &[Stmt], name: &str) -> usize {
    // `stmts` is the list that DECLARES the name (the premise, not
    // a shadow), so no same-level declaration check here.
    let mut sites: Vec<Site> = Vec::new();
    for s in stmts {
        collect_stmt_fns_only(ast, s, name, &mut sites);
    }
    apply_sites(ast, &sites, true);
    sites.len()
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
    match s {
        Stmt::FnDecl { .. } => {
            collect_stmt(ast, s, sn, out);
            return;
        }
        Stmt::Block(list) | Stmt::Multi(list) => {
            if list_declares(list, sn) {
                return;
            }
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } if var_name == sn => {
            return;
        }
        Stmt::For {
            init: Some(init), ..
        } if for_head_declares(init, sn) => return,
        Stmt::Try {
            catch_param: Some(c),
            ..
        } if c == sn => return,
        _ => {}
    }
    for e in stmt_exprs(s) {
        collect_expr_fns_only(ast, e, sn, out);
    }
    for c in stmt_children_ref(s) {
        collect_stmt_fns_only(ast, c, sn, out);
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
/// `var` anywhere in it, or a let / function / class at its top)?
fn list_declares(list: &[Stmt], sn: &str) -> bool {
    let mut vars: Vec<String> = Vec::new();
    hoist_var_names(list, &mut vars);
    vars.iter().any(|v| v == sn)
        || list.iter().any(|s| {
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
    match s {
        Stmt::FnDecl { params, body, .. } => {
            if !rebinds(params, body, sn) {
                for b in body {
                    collect_stmt(ast, b, sn, out);
                }
            }
            return;
        }
        Stmt::Block(list) | Stmt::Multi(list) => {
            if list_declares(list, sn) {
                return;
            }
        }
        // A for-of head names a fresh per-iteration binding.
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } if var_name == sn => {
            return;
        }
        Stmt::For {
            init: Some(init), ..
        } if for_head_declares(init, sn) => return,
        Stmt::Try {
            catch_param: Some(c),
            ..
        } if c == sn => return,
        _ => {}
    }
    for e in stmt_exprs(s) {
        collect_expr(ast, e, sn, out);
    }
    for c in stmt_children_ref(s) {
        collect_stmt(ast, c, sn, out);
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
        Expr::ArrowFn { params, body, .. } => {
            if !rebinds(params, body, sn) {
                for s in body {
                    collect_stmt(ast, s, sn, out);
                }
            }
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, sn, out);
    }
}
