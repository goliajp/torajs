//! Per-iteration bindings of a `for (let …)` loop in a generator body.
//!
//! §14.7.4.2 ForBodyEvaluation copies the loop's `let` binding into a
//! fresh environment before every iteration
//! (CreatePerIterationEnvironment), so a closure made in one iteration
//! keeps that iteration's binding. The let-lift turns the binding into
//! one state-machine field, which every closure then shares. When a
//! closure in the loop reads the binding, it is boxed instead: the
//! field holds a one-element array, the cell of the current iteration;
//! the step expression first replaces it with a copy, then steps the
//! copy; every plain reference reads and writes `cell[0]`; and every
//! closure reading the binding captures the cell that is current when
//! it is made, never the field.
//!
//! Each statement that makes such a closure is preceded by a local
//! holding the current cell, which the closure captures in place of the
//! field:
//!
//! ```text
//! for (let i = 0; i < n; i++) fs.push(() => i)
//! for (let c = [0]; c[0] < n; c = [c[0]], c[0]++) {
//!     const k = c;
//!     fs.push(() => k[0]);
//! }
//! ```
//!
//! That local is a plain local of one state arm (`LiftCtx::arm_locals`
//! keeps it out of the lift), so it has to be evaluated in the arm that
//! makes the closure. A loop test or step runs again on every pass, so
//! one that makes such a closure — the boxed loop's own or a nested
//! loop's — is first moved into the loop body (`rotate`), where it is a
//! statement of its own. A later declaration of the boxed loop's head
//! that makes one takes its local inside the head, and the first
//! iteration then starts from a copy of the cell the head made, the
//! copy §14.7.4.2 makes before the first iteration. A statement that
//! suspends before it is done evaluating its own expressions has no
//! arm for the local; a loop where one makes such a closure is left as
//! it was.

mod block_let;
mod class_binding;
mod class_member;
mod rotate;
mod scan;

pub(super) use block_let::box_captured_block_lets;

use super::LiftCtx;
use crate::ast::desugar_generators_sm_rewrite::stmt_contains_yield;
use crate::ast::desugar_with::walk::{stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use scan::{
    arrow_reads, arrows_in, collect_deep, decl_reads, head_decls, head_decls_mut,
    idents_outside_arrows, mentions, own_arrows, stmt_roots,
};
use std::collections::HashMap;

/// What the rewrite of one loop needs at every closure site.
struct Cell {
    /// The lifted field holding the current iteration's cell.
    field: String,
    /// What the binding's references were renamed to.
    marker: String,
    /// The type of what the field holds: the cell's, `Array<T>`, or
    /// the binding's own when it is not boxed.
    ty: String,
    /// What the arm locals and rotation flags are named after.
    stem: String,
    /// Arm locals and rotation flags minted so far.
    locals: usize,
    /// Whether the field holds a cell. A binding nothing writes after
    /// its declaration is not boxed: its arm locals hold the value.
    boxed: bool,
}

/// Box each binding of the `for (let …)` at `s` that a closure in the
/// loop reads. `taken` is what a lifted field may not be called.
pub(super) fn box_captured_for_let(
    ast: &mut Ast,
    s: &mut Stmt,
    taken: &[(String, String)],
    ctx: &mut LiftCtx,
) {
    let Stmt::For { init: Some(i), .. } = s else {
        return;
    };
    // by name: boxing one binding adds statements to the head
    let names: Vec<String> = head_decls(i)
        .into_iter()
        .filter_map(|d| match d {
            Stmt::LetDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    for name in names {
        let Stmt::For { init: Some(i), .. } = s else {
            return;
        };
        let at = head_decls(i)
            .into_iter()
            .position(|d| matches!(d, Stmt::LetDecl { name: n, .. } if *n == name));
        if let Some(k) = at
            && let Some(binding) = boxable(ast, s, k)
        {
            box_one(ast, s, k, binding, taken, ctx);
        }
    }
}

/// Box the binding declared by the `k`th statement of the head.
fn box_one(
    ast: &mut Ast,
    s: &mut Stmt,
    k: usize,
    (name, type_ann, first): (String, Option<String>, ExprId),
    taken: &[(String, String)],
    ctx: &mut LiftCtx,
) {
    let field = mint(&format!("{name}__iter"), taken);
    let marker = format!("{field}__ref");
    let map = HashMap::from([(name.clone(), marker.clone())]);
    crate::ast::nested_fns_idents::rewrite_idents_in_body(ast, std::slice::from_mut(s), &map, true);
    class_member::rename_in_parts(ast, s, &map);
    let elem = super::local_ann::local_ann(ast, &name, type_ann.as_ref(), first, ctx);
    let mut cell = Cell {
        stem: field.clone(),
        field,
        marker,
        ty: format!("Array<{elem}>"),
        locals: 0,
        boxed: true,
    };
    let Stmt::For {
        init: Some(init),
        cond,
        step,
        body,
    } = s
    else {
        return;
    };
    let mut roots: Vec<ExprId> = cond.iter().chain(step.iter()).copied().collect();
    stmt_roots(body, &mut roots);
    for d in head_decls(init).into_iter().skip(k + 1) {
        roots.extend(stmt_exprs(d));
    }
    for r in idents_outside_arrows(ast, &roots, &cell.marker) {
        let read = cell_read(ast, &cell.field);
        ast.exprs[r.0 as usize] = read;
    }
    rotate::rotate(ast, s, &mut cell);
    let Stmt::For {
        init: Some(init),
        step,
        body,
        ..
    } = s
    else {
        return;
    };
    bind_in_slot(ast, body, &mut cell, ctx);
    let first_cell = ast.add_expr(Expr::Array(vec![first]));
    let decl = Stmt::LetDecl {
        mutable: true,
        name: cell.field.clone(),
        type_ann: Some(cell.ty.clone()),
        init: first_cell,
        is_var: false,
    };
    *head_decls_mut(init).swap_remove(k) = decl;
    let copy = cell_copy(ast, &cell.field);
    *step = Some(match *step {
        Some(right) => ast.add_expr(Expr::Sequence { left: copy, right }),
        None => copy,
    });
    let Stmt::Multi(v) = init.as_mut() else {
        return;
    };
    let made = cell.locals;
    bind_in_list(ast, v, &mut cell, ctx);
    if cell.locals > made {
        v.push(Stmt::Expr(cell_copy(ast, &cell.field)));
    }
}

/// `field = [field[0]]`: the next iteration's cell.
fn cell_copy(ast: &mut Ast, field: &str) -> ExprId {
    let target = ast.add_expr(Expr::Ident(field.to_string()));
    let current = cell_read(ast, field);
    let current = ast.add_expr(current);
    let value = ast.add_expr(Expr::Array(vec![current]));
    ast.add_expr(Expr::Assign { target, value })
}

/// The `k`th head statement's binding, when it needs boxing: some
/// closure in the loop reads it.
fn boxable(ast: &Ast, s: &Stmt, k: usize) -> Option<(String, Option<String>, ExprId)> {
    let Stmt::For {
        init: Some(init),
        cond,
        step,
        body,
    } = s
    else {
        return None;
    };
    let decls = head_decls(init);
    let Stmt::LetDecl {
        name,
        type_ann,
        init: first,
        is_var: false,
        ..
    } = decls[k]
    else {
        return None;
    };
    // An inner declaration of the same name ends the rename's reach,
    // which the rename walk does not see; a function declared in the
    // body, being hoisted, has no statement of its own to take the
    // local; and a head declaration up to this one runs before there
    // is a cell to read. Such a loop is left alone.
    if class_member::rebinds_past_members(body, name)
        || decl_reads(ast, body, name)
        || decls[..=k].iter().any(|d| mentions(ast, d, name))
    {
        return None;
    }
    let mut roots: Vec<ExprId> = cond.iter().chain(step.iter()).copied().collect();
    stmt_roots(body, &mut roots);
    for d in &decls[k + 1..] {
        roots.extend(stmt_exprs(d));
    }
    let read = arrows_in(ast, &roots)
        .iter()
        .any(|&a| arrow_reads(ast, a, name))
        || class_member::class_reads(ast, body, name);
    (read && !unhosted(ast, body, name)).then(|| (name.clone(), type_ann.clone(), *first))
}

/// Whether a statement under `s` makes a closure reading `name` where
/// no local can stand in front of it.
fn unhosted(ast: &Ast, s: &Stmt, name: &str) -> bool {
    if !hostable(s)
        && own_arrows(ast, s)
            .iter()
            .any(|&a| arrow_reads(ast, a, name))
    {
        return true;
    }
    !matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. })
        && stmt_children_ref(s)
            .into_iter()
            .any(|c| unhosted(ast, c, name))
}

/// Whether a local placed right before `s` is still live where `s`
/// makes its closures: `s` evaluates its own expressions before it
/// suspends, if it does — a `yield` statement suspends after its
/// operand, an `if` after its test, a `switch` after its discriminant
/// and case tests — and a loop's own expressions run
/// once on entry, its test and step having been moved into its body
/// (`rotate`).
fn hostable(s: &Stmt) -> bool {
    match s {
        Stmt::Labeled { body, .. } => hostable(body),
        Stmt::For { .. }
        | Stmt::DoWhile { .. }
        | Stmt::While { .. }
        | Stmt::ForOf { .. }
        | Stmt::ForOfSplitIter { .. }
        | Stmt::If { .. }
        | Stmt::Switch { .. }
        | Stmt::Yield(_) => true,
        _ => !stmt_contains_yield(s),
    }
}

fn bind_in_list(ast: &mut Ast, list: &mut Vec<Stmt>, cell: &mut Cell, ctx: &mut LiftCtx) {
    let mut i = 0;
    while i < list.len() {
        rotate_loop(ast, &mut list[i], cell);
        if let Some(decl) = bind_own(ast, &list[i], cell, ctx) {
            list.insert(i, decl);
            i += 1;
        }
        bind_children(ast, &mut list[i], cell, ctx);
        i += 1;
    }
}

fn bind_in_slot(ast: &mut Ast, slot: &mut Stmt, cell: &mut Cell, ctx: &mut LiftCtx) {
    if let Stmt::Block(v) = slot {
        return bind_in_list(ast, v, cell, ctx);
    }
    rotate_loop(ast, slot, cell);
    match bind_own(ast, slot, cell, ctx) {
        Some(decl) => {
            let inner = std::mem::replace(slot, Stmt::Block(Vec::new()));
            let mut list = vec![decl, inner];
            bind_children(ast, &mut list[1], cell, ctx);
            *slot = Stmt::Block(list);
        }
        None => bind_children(ast, slot, cell, ctx),
    }
}

fn bind_children(ast: &mut Ast, s: &mut Stmt, cell: &mut Cell, ctx: &mut LiftCtx) {
    match s {
        Stmt::Block(v) | Stmt::Multi(v) => bind_in_list(ast, v, cell, ctx),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            bind_in_slot(ast, then_branch, cell, ctx);
            if let Some(e) = else_branch {
                bind_in_slot(ast, e, cell, ctx);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => bind_in_slot(ast, body, cell, ctx),
        Stmt::Labeled { body, .. } => bind_children(ast, body, cell, ctx),
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                bind_in_list(ast, &mut c.body, cell, ctx);
            }
            if let Some(d) = default {
                bind_in_list(ast, d, cell, ctx);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            bind_in_list(ast, body, cell, ctx);
            bind_in_list(ast, catch_body, cell, ctx);
            if let Some(f) = finally_body {
                bind_in_list(ast, f, cell, ctx);
            }
        }
        _ => {}
    }
}

/// [`rotate::rotate`] the loop `s` is, under its labels.
fn rotate_loop(ast: &mut Ast, s: &mut Stmt, cell: &mut Cell) {
    match s {
        Stmt::Labeled { body, .. } => rotate_loop(ast, body, cell),
        _ => rotate::rotate(ast, s, cell),
    }
}

/// When `s` itself makes a closure reading the binding: point the
/// closure at a fresh arm local and return that local's declaration.
fn bind_own(ast: &mut Ast, s: &Stmt, cell: &mut Cell, ctx: &mut LiftCtx) -> Option<Stmt> {
    let mut refs = class_member::own_class_refs(ast, s, &cell.marker);
    for a in own_arrows(ast, s) {
        collect_deep(ast, a, &cell.marker, &mut refs);
    }
    if refs.is_empty() {
        return None;
    }
    let local = format!("{}__k{}", cell.stem, cell.locals);
    cell.locals += 1;
    for r in refs {
        let read = if cell.boxed {
            cell_read(ast, &local)
        } else {
            Expr::Ident(local.clone())
        };
        ast.exprs[r.0 as usize] = read;
    }
    ctx.arm_locals.insert(local.clone());
    let init = ast.add_expr(Expr::Ident(cell.field.clone()));
    Some(Stmt::LetDecl {
        mutable: false,
        name: local,
        type_ann: Some(cell.ty.clone()),
        init,
        is_var: false,
    })
}

/// `<base>[0]`.
fn cell_read(ast: &mut Ast, base: &str) -> Expr {
    let obj = ast.add_expr(Expr::Ident(base.to_string()));
    let index = ast.add_expr(Expr::Number(0.0));
    Expr::Index { obj, index }
}

/// A field name nothing lifted has claimed.
fn mint(base: &str, taken: &[(String, String)]) -> String {
    let mut k = 0;
    loop {
        let candidate = format!("{base}{k}");
        if !taken.iter().any(|(n, _)| *n == candidate) {
            return candidate;
        }
        k += 1;
    }
}
