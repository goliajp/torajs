//! Per-iteration bindings of a `for (let …)` loop in a generator body.
//!
//! §14.7.4.2 ForBodyEvaluation copies the loop's `let` binding into a
//! fresh environment before every iteration
//! (CreatePerIterationEnvironment), so a closure made in one iteration
//! keeps that iteration's binding. The let-lift turns the binding into
//! one state-machine field, which every closure then shares. When a
//! closure in the loop body reads the binding, it is boxed instead: the
//! field holds a one-element array, the cell of the current iteration;
//! the step expression first replaces it with a copy, then steps the
//! copy; every plain reference reads and writes `cell[0]`; and each
//! statement that makes such a closure is preceded by a local holding
//! the current cell, which the closure captures in place of the field.
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
//! keeps it out of the lift), so the statement that makes the closure
//! must not itself suspend; a loop where one does is left as it was.

use super::LiftCtx;
use crate::ast::desugar_generators_sm_rewrite::stmt_contains_yield;
use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// What the rewrite of one loop needs at every closure site.
struct Cell {
    /// The lifted field holding the current iteration's cell.
    field: String,
    /// What the binding's references were renamed to.
    marker: String,
    /// The cell's type, `Array<T>`.
    ty: String,
    /// Arm locals minted so far.
    locals: usize,
}

/// Box the binding of the `for (let …)` at `s` when a closure in the
/// loop body reads it. `taken` is what a lifted field may not be called.
pub(super) fn box_captured_for_let(
    ast: &mut Ast,
    s: &mut Stmt,
    taken: &[(String, String)],
    ctx: &mut LiftCtx,
) {
    let Some((name, type_ann, first)) = boxable(ast, s) else {
        return;
    };
    let field = mint(&format!("{name}__iter"), taken);
    let marker = format!("{field}__ref");
    let map = HashMap::from([(name.clone(), marker.clone())]);
    crate::ast::nested_fns_idents::rewrite_idents_in_body(ast, std::slice::from_mut(s), &map, true);
    let elem = super::local_ann::local_ann(ast, &name, type_ann.as_ref(), first, ctx);
    let mut cell = Cell {
        field,
        marker,
        ty: format!("Array<{elem}>"),
        locals: 0,
    };
    let Stmt::For {
        init,
        cond,
        step,
        body,
    } = s
    else {
        return;
    };
    let mut roots: Vec<ExprId> = cond.iter().chain(step.iter()).copied().collect();
    stmt_roots(body, &mut roots);
    for r in idents_outside_arrows(ast, &roots, &cell.marker) {
        let read = cell_read(ast, &cell.field);
        ast.exprs[r.0 as usize] = read;
    }
    bind_in_slot(ast, body, &mut cell, ctx);
    let first_cell = ast.add_expr(Expr::Array(vec![first]));
    *init = Some(Box::new(Stmt::LetDecl {
        mutable: true,
        name: cell.field.clone(),
        type_ann: Some(cell.ty),
        init: first_cell,
        is_var: false,
    }));
    let target = ast.add_expr(Expr::Ident(cell.field.clone()));
    let current = cell_read(ast, &cell.field);
    let current = ast.add_expr(current);
    let value = ast.add_expr(Expr::Array(vec![current]));
    let copy = ast.add_expr(Expr::Assign { target, value });
    *step = Some(match *step {
        Some(right) => ast.add_expr(Expr::Sequence { left: copy, right }),
        None => copy,
    });
}

/// The binding of a `for (let …)` that needs boxing: some statement of
/// the body makes a closure reading it, and every such statement can
/// take a local in front of it.
fn boxable(ast: &Ast, s: &Stmt) -> Option<(String, Option<String>, ExprId)> {
    let Stmt::For {
        init: Some(init),
        cond,
        step,
        body,
    } = s
    else {
        return None;
    };
    let Stmt::LetDecl {
        name,
        type_ann,
        init: first,
        is_var: false,
        ..
    } = init.as_ref()
    else {
        return None;
    };
    // An inner declaration of the same name ends the rename's reach,
    // which the rename walk does not see; a function or class declared
    // in the body, or a closure in the test or step, has no statement
    // of its own to take the local. Such a loop is left alone.
    let head: Vec<ExprId> = cond.iter().chain(step.iter()).copied().collect();
    if crate::ast::rebinds_in_stmt(body, name)
        || decl_reads(ast, body, name)
        || arrows_in(ast, &head)
            .iter()
            .any(|&a| arrow_reads(ast, a, name))
    {
        return None;
    }
    match owners(ast, body, name) {
        Some(n) if n > 0 => Some((name.clone(), type_ann.clone(), *first)),
        _ => None,
    }
}

/// How many statements under `s` make a closure reading `name`; `None`
/// when one of them suspends or is a loop (whose head would have to be
/// re-evaluated per iteration, which a local in front cannot do).
fn owners(ast: &Ast, s: &Stmt, name: &str) -> Option<usize> {
    let mut n = 0;
    if own_arrows(ast, s)
        .iter()
        .any(|&a| arrow_reads(ast, a, name))
    {
        let is_loop = matches!(
            s,
            Stmt::For { .. }
                | Stmt::While { .. }
                | Stmt::DoWhile { .. }
                | Stmt::ForOf { .. }
                | Stmt::ForOfSplitIter { .. }
        );
        if is_loop || stmt_contains_yield(s) {
            return None;
        }
        n += 1;
    }
    if !matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
        for c in stmt_children_ref(s) {
            n += owners(ast, c, name)?;
        }
    }
    Some(n)
}

fn bind_in_list(ast: &mut Ast, list: &mut Vec<Stmt>, cell: &mut Cell, ctx: &mut LiftCtx) {
    let mut i = 0;
    while i < list.len() {
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

/// When `s` itself makes a closure reading the binding: point the
/// closure at a fresh arm local and return that local's declaration.
fn bind_own(ast: &mut Ast, s: &Stmt, cell: &mut Cell, ctx: &mut LiftCtx) -> Option<Stmt> {
    let mut refs = Vec::new();
    for a in own_arrows(ast, s) {
        collect_deep(ast, a, &cell.marker, &mut refs);
    }
    if refs.is_empty() {
        return None;
    }
    let local = format!("{}__k{}", cell.field, cell.locals);
    cell.locals += 1;
    for r in refs {
        let read = cell_read(ast, &local);
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

/// The expressions `s` evaluates itself — a `for` head's declaration
/// included — not its child statements'.
fn own_arrows(ast: &Ast, s: &Stmt) -> Vec<ExprId> {
    let mut roots = stmt_exprs(s);
    if let Stmt::For { init: Some(i), .. } = s {
        roots.extend(stmt_exprs(i));
    }
    arrows_in(ast, &roots)
}

fn stmt_roots(s: &Stmt, out: &mut Vec<ExprId>) {
    out.extend(stmt_exprs(s));
    for c in stmt_children_ref(s) {
        stmt_roots(c, out);
    }
}

/// Whether a function or class declared in the loop body reads `name`.
fn decl_reads(ast: &Ast, s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::FnDecl { .. } => {
            crate::ast::free_vars::free_vars_of_body(ast, &[], std::slice::from_ref(s))
                .iter()
                .any(|n| n == name)
        }
        Stmt::ClassDecl { .. } => true,
        _ => stmt_children_ref(s)
            .into_iter()
            .any(|c| decl_reads(ast, c, name)),
    }
}

/// The closures reachable from `roots` without entering one.
fn arrows_in(ast: &Ast, roots: &[ExprId]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(e) = stack.pop() {
        match ast.get_expr(e) {
            Expr::ArrowFn { .. } => out.push(e),
            _ => stack.extend(expr_children(ast, e)),
        }
    }
    out
}

fn arrow_reads(ast: &Ast, a: ExprId, name: &str) -> bool {
    let Expr::ArrowFn { params, body, .. } = ast.get_expr(a) else {
        return false;
    };
    let bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    crate::ast::free_vars::free_vars_of_body(ast, &bound, body)
        .iter()
        .any(|n| n == name)
}

/// `Ident(name)` nodes reachable from `roots` without entering a
/// closure.
fn idents_outside_arrows(ast: &Ast, roots: &[ExprId], name: &str) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(e) = stack.pop() {
        match ast.get_expr(e) {
            Expr::Ident(n) if n == name => out.push(e),
            Expr::ArrowFn { .. } => {}
            _ => stack.extend(expr_children(ast, e)),
        }
    }
    out
}

/// Every `Ident(name)` under `e`, closures and the function
/// declarations inside them included. The rename already stopped
/// where a nested scope rebinds the name.
fn collect_deep(ast: &Ast, e: ExprId, name: &str, out: &mut Vec<ExprId>) {
    match ast.get_expr(e) {
        Expr::Ident(n) if n == name => out.push(e),
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_deep_stmt(ast, s, name, out);
            }
        }
        _ => {
            for c in expr_children(ast, e) {
                collect_deep(ast, c, name, out);
            }
        }
    }
}

fn collect_deep_stmt(ast: &Ast, s: &Stmt, name: &str, out: &mut Vec<ExprId>) {
    for e in stmt_exprs(s) {
        collect_deep(ast, e, name, out);
    }
    let children: Vec<&Stmt> = match s {
        Stmt::FnDecl { body, .. } => body.iter().collect(),
        _ => stmt_children_ref(s),
    };
    for c in children {
        collect_deep_stmt(ast, c, name, out);
    }
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
