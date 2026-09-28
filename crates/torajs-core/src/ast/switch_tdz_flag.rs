//! §14.12.4 — the temporal dead zone of a `let` / `const` declared in a
//! switch's CaseBlock. The case clauses share one declarative
//! environment, so a binding declared in one clause is visible in every
//! other, but it is initialized only when control runs through its
//! declaration:
//!
//! ```text
//! switch (x) {
//!   case 1: let s = 1; …
//!   case 2: s = 5;        // x === 2: ReferenceError
//! }
//! ```
//!
//! tr answered 5 here, and read uninitialized storage for a `const`.
//! No statement order settles it: entering at `case 2` skips the
//! declaration, falling through from `case 1` runs it. So the binding
//! gets the runtime initialised-flag `closure_tdz_flag` gives a closure
//! read (the same site walk and rewrite), declared right before the
//! switch so every execution of the statement starts it false.
//!
//! The one region that needs no flag is the rest of the declaration's
//! own clause: control enters a clause only at its start, so a
//! statement written after the declaration in the same clause runs
//! after it. Everything else in the CaseBlock is a site — the other
//! clauses, the statements before the declaration, its own
//! initializer, and the case selectors, which §14.12.4 evaluates in the
//! block's environment before any body runs. A statement that binds
//! the name again anywhere inside it is left alone, as `tdz_reads`
//! does: its occurrences may be the inner binding's, and skipping them
//! can only miss a raise, never invent one.

use std::collections::HashSet;

use super::closure_tdz_flag::{at, at_mut, flat_paths, wrap};
use super::desugar_generators_tdz_flag::{
    Site, collect_sites_expr, collect_sites_stmt, rewrite_site,
};
use super::tdz_reads::collect_bound_names;
use super::{Ast, Expr, Stmt, SwitchCase};

/// Flag the CaseBlock's bindings that some site can reach before their
/// declaration runs. Answers the flag declarations the caller places
/// in front of the switch statement.
pub(super) fn flag_case_block(
    ast: &mut Ast,
    cases: &mut [SwitchCase],
    default: &mut Option<Vec<Stmt>>,
    default_at: usize,
    counter: &mut usize,
) -> Vec<Stmt> {
    let selectors: Vec<_> = cases.iter().map(|c| c.value).collect();
    let (before, after) = cases.split_at_mut(default_at);
    let mut bodies: Vec<&mut Vec<Stmt>> = before.iter_mut().map(|c| &mut c.body).collect();
    bodies.extend(default.as_mut());
    bodies.extend(after.iter_mut().map(|c| &mut c.body));
    let paths: Vec<Vec<Vec<usize>>> = bodies
        .iter()
        .map(|b| {
            let mut out = Vec::new();
            flat_paths(b, &mut Vec::new(), &mut out);
            out
        })
        .collect();
    let mut flagged: Vec<(usize, usize, String)> = Vec::new();
    for (ci, clause) in paths.iter().enumerate() {
        for (k, path) in clause.iter().enumerate() {
            let Stmt::LetDecl {
                name,
                init,
                is_var: false,
                ..
            } = at(bodies[ci], path)
            else {
                continue;
            };
            if name.starts_with("__") {
                continue;
            }
            let (name, init) = (name.clone(), *init);
            let mut sites: Vec<Site> = Vec::new();
            for sel in &selectors {
                collect_sites_expr(ast, *sel, &name, true, true, &mut sites);
            }
            collect_sites_expr(ast, init, &name, true, true, &mut sites);
            for (cj, other) in paths.iter().enumerate() {
                for (kj, p) in other.iter().enumerate() {
                    if cj == ci && kj >= k {
                        break;
                    }
                    let s = at(bodies[cj], p);
                    let mut bound = HashSet::new();
                    collect_bound_names(s, &mut bound);
                    if !bound.contains(name.as_str()) {
                        collect_sites_stmt(ast, s, &name, true, true, &mut sites);
                    }
                }
            }
            if sites.is_empty() {
                continue;
            }
            *counter += 1;
            let flag = format!("__tdzs{}_{name}", *counter);
            for site in sites {
                rewrite_site(ast, site, &name, &flag);
            }
            flagged.push((ci, k, flag));
        }
    }
    let mut heads = Vec::with_capacity(flagged.len());
    for (ci, k, flag) in flagged {
        let target = ast.add_expr(Expr::Ident(flag.clone()));
        let value = ast.add_expr(Expr::Bool(true));
        let set = Stmt::Expr(ast.add_expr(Expr::Assign { target, value }));
        wrap(at_mut(bodies[ci], &paths[ci][k]), |s| vec![s, set]);
        let init = ast.add_expr(Expr::Bool(false));
        heads.push(Stmt::LetDecl {
            mutable: true,
            name: flag,
            type_ann: Some("boolean".into()),
            init,
            is_var: false,
        });
    }
    heads
}

/// A generator or async machine body's switches, flagged before the
/// let-lift turns their declarations into field stores (after it no
/// `let` is left for the ordinary pass to find). Nested functions are
/// not entered: they are ordinary bodies, flagged by that pass.
pub(super) fn flag_machine_switches(ast: &mut Ast, stmts: &mut [Stmt], counter: &mut usize) {
    for s in stmts.iter_mut() {
        flag_machine_stmt(ast, s, counter);
    }
}

fn flag_machine_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    match s {
        Stmt::Switch {
            cases,
            default,
            default_at,
            ..
        } => {
            for c in cases.iter_mut() {
                flag_machine_switches(ast, &mut c.body, counter);
            }
            if let Some(d) = default {
                flag_machine_switches(ast, d, counter);
            }
            let heads = flag_case_block(ast, cases, default, *default_at, counter);
            if !heads.is_empty() {
                wrap(s, |sw| heads.into_iter().chain([sw]).collect());
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => flag_machine_switches(ast, v, counter),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            flag_machine_stmt(ast, then_branch, counter);
            if let Some(e) = else_branch {
                flag_machine_stmt(ast, e, counter);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::For { body, .. } => flag_machine_stmt(ast, body, counter),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            flag_machine_switches(ast, body, counter);
            flag_machine_switches(ast, catch_body, counter);
            if let Some(f) = finally_body {
                flag_machine_switches(ast, f, counter);
            }
        }
        _ => {}
    }
}
