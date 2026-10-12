//! A capturing `function` declared directly in a `switch` clause.
//!
//! The router rewrites it into a `let` holding a function expression
//! where it stood. In a switch that leaves the binding in one clause,
//! and a clause the switch jumps to reads it in its dead zone — while
//! §14.12.4 initializes every function of the CaseBlock on entry.
//! Moved into a block wrapped around the switch, under a fresh name the
//! selector and clause references follow, it is initialized before any
//! clause runs, the shape `nested_fns_case_block` gives the declarations
//! that do not capture.
//!
//! The move is sound only for a declaration whose captures all live
//! outside the CaseBlock: one reading a lexical binding of some clause,
//! or a function of one that keeps the declaration form, would leave
//! that binding's scope. Those stay where they are — the hoisting
//! boundary the router's own doc records.

use super::walk::{Scopes, walk_children};
use super::{EarlyUse, Routed, raise_early, route_list};
use crate::ast::annexb_block_binding::split_block_binding;
use crate::ast::annexb_fn_var::annexb_applies_at;
use crate::ast::nested_fns_case_block::{clauses, rename_case_block};
use crate::ast::desugar_async_nested_fns::rename::rename_expr;
use crate::ast::scope_chain::list_names;
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashMap;

/// Route every clause of the switch `s`, then move the routed
/// declarations that can be moved in front of it.
pub(super) fn walk_case_block(
    ast: &mut Ast,
    s: &mut Stmt,
    globals: &[String],
    scope: &mut Scopes,
    counter: &mut u32,
) {
    let Stmt::Switch {
        cases,
        default,
        default_at,
        ..
    } = s
    else {
        unreachable!("caller matched a switch");
    };
    let mut bodies = clauses(cases, default, *default_at);
    // One block holds every clause.
    for b in bodies.iter() {
        list_names(b, &mut scope.shadow);
    }
    let mut routed: Sites = Vec::new();
    let mut early: Vec<Option<EarlyUse>> = Vec::new();
    for (ci, body) in bodies.iter_mut().enumerate() {
        let depth = scope.mark();
        for st in body.iter_mut() {
            walk_children(ast, st, globals, scope, true, counter);
            if let Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } = st {
                scope.annexb.push(name.clone());
            }
        }
        let (sites, e) = route_list(ast, body, globals, scope, true);
        routed.extend(sites.into_iter().map(|r| (ci, r)));
        early.push(e);
        scope.reset(depth);
    }
    let held: Vec<String> = bodies
        .iter()
        .flat_map(|b| b.iter())
        .flat_map(clause_binding)
        .filter(|n| !routed.iter().any(|(_, r)| r.name == *n))
        .collect();
    let (movable, staying) = movable(routed, &held);
    // what stays gets the in-place §B.3.3 split a block's routed
    // declaration gets (`walk_list`), or moves up its clause whole when
    // a statement above it reaches it
    for (ci, r) in &staying {
        if !r.early && !scope.annexb.iter().any(|b| *b == r.name) {
            split_block_binding(ast, bodies[*ci], r.idx, &r.name, r.span, counter);
        }
    }
    let mut heads: Vec<Stmt> = Vec::new();
    let mut slots: Vec<(String, String)> = Vec::new();
    for (ci, r) in &movable {
        let slot = format!("__swfn_{}_{}", r.name, counter);
        *counter += 1;
        // §B.3.3's var binding, written where the declaration sits; an
        // enclosing binding of the name is step 1.a.ii's early error
        let site = if annexb_applies_at(ast, &r.name, r.span)
            && !scope.annexb.iter().any(|b| *b == r.name)
        {
            let init = ast.add_expr(Expr::Ident(slot.clone()));
            ast.set_expr_span(init, r.span);
            Stmt::LetDecl {
                mutable: true,
                name: r.name.clone(),
                type_ann: None,
                init,
                is_var: true,
            }
        } else {
            Stmt::Block(Vec::new())
        };
        let mut decl = std::mem::replace(&mut bodies[*ci][r.idx], site);
        if let Stmt::LetDecl { name, mutable, .. } = &mut decl {
            *name = slot.clone();
            // an ordinary let-like binding the body may assign
            *mutable = true;
        }
        heads.push(decl);
        slots.push((r.name.clone(), slot));
    }
    // raised only after the moves: a move swaps one statement for its
    // site in place, while a raise rotates the clause and would leave
    // the moves' indices pointing at other statements
    for (ci, e) in early.into_iter().enumerate() {
        let stays = |i: usize| staying.iter().any(|(c, r)| *c == ci && r.idx == i);
        if let Some(e) = e.and_then(|e| e.retain(stays)) {
            raise_early(bodies[ci], &e);
        }
    }
    if movable.is_empty() {
        return;
    }
    rename_case_block(ast, cases, default, &slots);
    // the moved bodies name themselves and each other
    let map: HashMap<String, String> = slots.into_iter().collect();
    for h in &heads {
        if let Stmt::LetDecl { init, .. } = h {
            rename_expr(ast, *init, &map);
        }
    }
    heads.push(std::mem::replace(s, Stmt::Block(Vec::new())));
    *s = Stmt::Block(heads);
}

/// A name a clause statement binds for the whole CaseBlock.
fn clause_binding(s: &Stmt) -> Vec<String> {
    match s {
        Stmt::LetDecl {
            name,
            is_var: false,
            ..
        }
        | Stmt::UsingDecl { name, .. }
        | Stmt::ClassDecl { name, .. }
        | Stmt::FnDecl { name, .. } => vec![name.clone()],
        Stmt::Multi(inner) => inner.iter().flat_map(clause_binding).collect(),
        _ => Vec::new(),
    }
}

/// Routed declarations, each with the index of its clause.
type Sites = Vec<(usize, Routed)>;

/// Split the routed declarations into those none of whose captures is
/// held by the CaseBlock, and the rest. One that stays is held too, for
/// the peers that capture it; a name declared twice stays in every
/// clause, since one slot cannot hold both.
fn movable(routed: Sites, held: &[String]) -> (Sites, Sites) {
    let all: Vec<String> = routed.iter().map(|(_, r)| r.name.clone()).collect();
    let (mut moving, mut staying): (Sites, Sites) = routed
        .into_iter()
        .partition(|(_, r)| all.iter().filter(|n| **n == r.name).count() == 1);
    loop {
        let (keep, drop): (Sites, Sites) = moving.into_iter().partition(|(_, r)| {
            !r.free
                .iter()
                .any(|n| held.contains(n) || staying.iter().any(|(_, s)| s.name == *n))
        });
        moving = keep;
        if drop.is_empty() {
            return (moving, staying);
        }
        staying.extend(drop);
    }
}
