//! Block bindings of a statement list that runs more than once.
//!
//! §14.2.2 evaluates a Block in a fresh declarative environment every
//! time, so a closure made on one pass of a loop body keeps that pass's
//! `let` / `const` binding. The let-lift gives the binding one
//! state-machine field, which every closure then shares. When a closure
//! after the declaration reads the binding, the statement making it is
//! preceded by an arm local the closure reads instead, as for a
//! `for (let …)` binding:
//!
//! - a binding nothing assigns after its declaration — every `const`,
//!   most `let`s — needs no cell: the local holds its value;
//! - one that is assigned is boxed: the declaration stores a fresh
//!   one-element array each time it runs (the new environment), plain
//!   references read and write `field[0]`, and the local holds the
//!   current cell.
//!
//! ```text
//! while (c) { const n = f(); yield; fs.push(() => n) }
//! while (c) { this.n = f(); yield; const k = this.n; fs.push(() => k) }
//! ```
//!
//! This runs on a list after the lift, so the declaration is already
//! the field's initializing store and a name the list shares with
//! another scope already has its own field.

use super::class_member::{class_assigns, class_reads, rebinds_past_members, rename_in_classes};
use super::scan::{
    arrow_reads, arrows_in, assigns_deep, decl_reads, idents_outside_arrows, mentions, stmt_roots,
};
use super::{Cell, LiftCtx, bind_children, bind_own, cell_read, rotate_loop, unhosted};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// Give each lifted block binding of `list` that a later closure reads
/// a per-pass local.
pub(in crate::ast::desugar_generators_walkers) fn box_captured_block_lets(
    ast: &mut Ast,
    list: &mut [Stmt],
    lifted: &mut [(String, String)],
    ctx: &mut LiftCtx,
) {
    for i in 0..list.len() {
        let (head, rest) = list.split_at_mut(i + 1);
        let (before, at) = head.split_at_mut(i);
        let at = &mut at[0];
        let stores: Vec<(ExprId, String)> = members(at)
            .iter()
            .filter_map(|s| lifted_store(ast, s, ctx))
            .collect();
        for (store, name) in stores {
            let mut after: Vec<&mut Stmt> = Vec::new();
            if let Stmt::Multi(v) = &mut *at {
                // an earlier member's boxing may have wrapped this one
                let Some(p) = v
                    .iter()
                    .position(|s| matches!(s, Stmt::Expr(e) if *e == store))
                else {
                    continue;
                };
                after.extend(v[p + 1..].iter_mut());
            }
            after.extend(rest.iter_mut());
            box_one(ast, before, store, &name, after, lifted, ctx);
        }
    }
}

/// `s` itself, or the members of the `Multi` it is.
fn members(s: &Stmt) -> Vec<&Stmt> {
    match s {
        Stmt::Multi(v) => v.iter().collect(),
        s => vec![s],
    }
}

/// The store and the name of the binding whose lifted declaration `s`
/// is.
fn lifted_store(ast: &Ast, s: &Stmt, ctx: &LiftCtx) -> Option<(ExprId, String)> {
    let Stmt::Expr(e) = s else {
        return None;
    };
    if !ast.lifted_init_assigns.contains(e) {
        return None;
    }
    let Expr::Assign { target, .. } = ast.get_expr(*e) else {
        return None;
    };
    match ast.get_expr(*target) {
        Expr::Member { obj, name }
            if matches!(ast.get_expr(*obj), Expr::This) && !ctx.vars.contains_key(name) =>
        {
            Some((*e, name.clone()))
        }
        _ => None,
    }
}

fn box_one(
    ast: &mut Ast,
    before: &[Stmt],
    store: ExprId,
    name: &str,
    mut after: Vec<&mut Stmt>,
    lifted: &mut [(String, String)],
    ctx: &mut LiftCtx,
) {
    let Expr::Assign { target, value } = ast.get_expr(store).clone() else {
        return;
    };
    let mut roots = Vec::new();
    for s in after.iter() {
        stmt_roots(s, &mut roots);
    }
    // A closure made before the declaration, or by its own initializer,
    // reads the binding before there is a value to hold; a function
    // declared after it, being hoisted, has no statement of its own to
    // take the local; and a scope that declares the name again ends
    // the rename's reach. Such a binding is left alone.
    let read = arrows_in(ast, &roots)
        .iter()
        .any(|&a| arrow_reads(ast, a, name))
        || after.iter().any(|s| class_reads(ast, s, name));
    if !read
        || mentions(ast, &Stmt::Expr(value), name)
        || before.iter().any(|s| {
            reads_under(ast, s, name) || decl_reads(ast, s, name) || class_reads(ast, s, name)
        })
        || after.iter().any(|s| {
            rebinds_past_members(s, name) || decl_reads(ast, s, name) || unhosted(ast, s, name)
        })
    {
        return;
    }
    let Some(slot) = lifted.iter_mut().find(|(n, _)| n == name) else {
        return;
    };
    let boxed =
        assigns_deep(ast, &roots, name) || after.iter().any(|s| class_assigns(ast, s, name));
    let stem = format!("{name}__blk{}", ctx.cells);
    ctx.cells += 1;
    let marker = format!("{stem}__ref");
    let map = HashMap::from([(name.to_string(), marker.clone())]);
    for s in after.iter_mut() {
        crate::ast::nested_fns_idents::rewrite_idents_in_stmt(ast, s, &map, true);
        rename_in_classes(ast, s, &map);
    }
    if boxed {
        slot.1 = format!("Array<{}>", slot.1);
        ctx.binds.insert(name.to_string(), slot.1.clone());
        let value = ast.add_expr(Expr::Array(vec![value]));
        ast.exprs[store.0 as usize] = Expr::Assign { target, value };
    }
    let mut cell = Cell {
        field: name.to_string(),
        marker,
        ty: slot.1.clone(),
        stem,
        locals: 0,
        boxed,
    };
    for r in idents_outside_arrows(ast, &roots, &cell.marker) {
        ast.exprs[r.0 as usize] = if boxed {
            cell_read(ast, name)
        } else {
            Expr::Ident(name.to_string())
        };
    }
    for s in after {
        rotate_loop(ast, s, &mut cell);
        let decl = bind_own(ast, s, &mut cell, ctx);
        bind_children(ast, s, &mut cell, ctx);
        if let Some(decl) = decl {
            let inner = std::mem::replace(s, Stmt::Block(Vec::new()));
            *s = Stmt::Multi(vec![decl, inner]);
        }
    }
}

/// Whether `name` occurs anywhere under `s`, closures included.
fn reads_under(ast: &Ast, s: &Stmt, name: &str) -> bool {
    let mut roots = Vec::new();
    stmt_roots(s, &mut roots);
    roots
        .into_iter()
        .any(|r| mentions(ast, &Stmt::Expr(r), name))
}
