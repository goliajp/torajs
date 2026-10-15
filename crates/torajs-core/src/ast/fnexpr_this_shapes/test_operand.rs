//! The bare name in a TEST position — the condition of `if` /
//! `while` / `do … while` / `for` / `? :`, the operand of `!`, and an
//! operand of `&&` / `||` that itself stands in such a position.
//!
//! Never-calls proof family (the parent module doc, kind 1): a test
//! applies ToBoolean (§7.1.2), which answers `true` for any object
//! without reading a property or invoking anything. The value
//! of a `&&` / `||` in test position is consumed by that same
//! ToBoolean, so its operands are tests too; elsewhere the operator
//! hands one operand on as a value, which this shape does not admit.

use crate::ast::desugar_with::walk::stmt_children_ref;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::{BinOp, Expr, ExprId, Stmt, UnaryOp};

pub(super) fn test_operand_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let mut roots: Vec<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Ternary { cond, .. } => Some(*cond),
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => Some(*expr),
            _ => None,
        })
        .collect();
    let mut todo: Vec<&Stmt> = stmts.iter().collect();
    while let Some(s) = todo.pop() {
        match s {
            Stmt::If { cond, .. } | Stmt::While { cond, .. } | Stmt::DoWhile { cond, .. } => {
                roots.push(*cond)
            }
            Stmt::For { cond: Some(c), .. } => roots.push(*c),
            Stmt::FnDecl { body, .. } => todo.extend(body.iter()),
            _ => {}
        }
        todo.extend(stmt_children_ref(s));
    }
    let mut out = std::collections::HashSet::new();
    while let Some(r) = roots.pop() {
        let r = peel_as(exprs, r);
        match &exprs[r.0 as usize] {
            Expr::Ident(_) => {
                out.insert(r);
            }
            Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left,
                right,
            } => roots.extend([*left, *right]),
            _ => {}
        }
    }
    out
}
