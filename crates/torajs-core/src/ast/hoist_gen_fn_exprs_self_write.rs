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

use super::desugar_with::walk::{stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, Stmt, UnaryOp};

mod scope_sites;

use scope_sites::{
    Site, collect_expr_fns_only, collect_stmt, collect_stmt_fns_only, list_declares,
};

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
