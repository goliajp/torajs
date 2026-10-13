//! A loop test or step that makes a closure reading the boxed binding
//! runs again on every pass, so no local in front of the loop can serve
//! it. The loop is rotated so that each becomes a statement of the body
//! — the shape the parser already gives a loop whose step yields:
//!
//! ```text
//! for (I; C; T) S  →  for (I, go = false; ; ) { if (go) T; go = true; if (!C) break; S }
//! for (I; C; T) S  →  for (I; ; T) { if (!C) break; S }      (only C makes one)
//! while (C) S      →  while (true) { if (!C) break; S }
//! do S while (C)   →  for (go = false; ; ) { if (go) { if (!C) break; } go = true; S }
//! ```
//!
//! `continue` still reaches the step — for the boxed loop itself, the
//! cell copy alone — and then the top of the body, which runs `T` and
//! `C` in their order.

use super::Cell;
use super::scan::{arrows_in, collect_deep};
use crate::ast::{Ast, Expr, ExprId, Stmt, UnaryOp};

/// Rotate the loop at `s` when its test or step makes a closure reading
/// the binding.
pub(super) fn rotate(ast: &mut Ast, s: &mut Stmt, cell: &mut Cell) {
    match s {
        Stmt::While { cond, body } if makes_reader(ast, *cond, cell) => {
            let c = std::mem::replace(cond, ast.add_expr(Expr::Bool(true)));
            let head = vec![break_unless(ast, c)];
            prepend(body, head);
        }
        Stmt::DoWhile { cond, body } if makes_reader(ast, *cond, cell) => {
            let test = break_unless(ast, *cond);
            let (decl, mut head) = later_passes(ast, Stmt::Block(vec![test]), cell);
            head.push(std::mem::replace(body.as_mut(), Stmt::Block(Vec::new())));
            *s = Stmt::For {
                init: Some(Box::new(decl)),
                cond: None,
                step: None,
                body: Box::new(Stmt::Block(head)),
            };
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let step_reads = step.is_some_and(|t| makes_reader(ast, t, cell));
            if !step_reads && !cond.is_some_and(|c| makes_reader(ast, c, cell)) {
                return;
            }
            let mut head = Vec::new();
            if let Some(t) = step.take_if(|_| step_reads) {
                let (decl, h) = later_passes(ast, Stmt::Expr(t), cell);
                *init = Some(Box::new(match init.take() {
                    Some(i) => Stmt::Multi(vec![*i, decl]),
                    None => decl,
                }));
                head = h;
            }
            if let Some(c) = cond.take() {
                head.push(break_unless(ast, c));
            }
            prepend(body, head);
        }
        _ => {}
    }
}

/// A fresh `go = false` declaration for the head, and the statements
/// running `then` on every pass but the first: `if (go) then; go =
/// true;`.
fn later_passes(ast: &mut Ast, then: Stmt, cell: &mut Cell) -> (Stmt, Vec<Stmt>) {
    let go = format!("{}__go{}", cell.stem, cell.locals);
    cell.locals += 1;
    let no = ast.add_expr(Expr::Bool(false));
    let decl = Stmt::LetDecl {
        mutable: true,
        name: go.clone(),
        type_ann: Some("boolean".into()),
        init: no,
        is_var: false,
    };
    let read = ast.add_expr(Expr::Ident(go.clone()));
    let target = ast.add_expr(Expr::Ident(go));
    let yes = ast.add_expr(Expr::Bool(true));
    let set = ast.add_expr(Expr::Assign { target, value: yes });
    let head = vec![
        Stmt::If {
            cond: read,
            then_branch: Box::new(then),
            else_branch: None,
        },
        Stmt::Expr(set),
    ];
    (decl, head)
}

/// `if (!c) break;`
fn break_unless(ast: &mut Ast, c: ExprId) -> Stmt {
    let not = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: c,
    });
    Stmt::If {
        cond: not,
        then_branch: Box::new(Stmt::Break(None)),
        else_branch: None,
    }
}

fn prepend(body: &mut Stmt, mut head: Vec<Stmt>) {
    head.push(std::mem::replace(body, Stmt::Block(Vec::new())));
    *body = Stmt::Block(head);
}

/// Whether `e` makes a closure reading the binding.
pub(super) fn makes_reader(ast: &Ast, e: ExprId, cell: &Cell) -> bool {
    let mut refs = Vec::new();
    for a in arrows_in(ast, &[e]) {
        collect_deep(ast, a, &cell.marker, &mut refs);
    }
    !refs.is_empty()
}
