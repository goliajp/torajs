//! The callee shapes the spelled-callee rewrite reads: which
//! operands lead down to the callee, and what sits at their leaves.

use super::receiver_names::Receivers;
use crate::ast::desugar_with::walk::expr_children;
use crate::ast::{Ast, Expr, ExprId};

/// A comma or conditional on the way down to the callee.
pub(super) fn spelled(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Sequence { .. } | Expr::Ternary { .. } => true,
        Expr::As { expr, .. } => spelled(ast, *expr),
        _ => false,
    }
}

pub(super) fn leaves_are_names(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Sequence { right, .. } => leaves_are_names(ast, *right),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => leaves_are_names(ast, *then_branch) && leaves_are_names(ast, *else_branch),
        Expr::As { expr, .. } => leaves_are_names(ast, *expr),
        Expr::Ident(n) => n != "eval",
        _ => false,
    }
}

pub(super) fn names_a_leaf(ast: &Ast, e: ExprId, wanted: &Receivers) -> bool {
    match ast.get_expr(e) {
        Expr::Sequence { right, .. } => names_a_leaf(ast, *right, wanted),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => names_a_leaf(ast, *then_branch, wanted) || names_a_leaf(ast, *else_branch, wanted),
        Expr::As { expr, .. } => names_a_leaf(ast, *expr, wanted),
        Expr::Ident(_) => wanted.names(ast, e),
        _ => false,
    }
}

pub(super) fn leaf_count(ast: &Ast, e: ExprId) -> usize {
    match ast.get_expr(e) {
        Expr::Sequence { right, .. } => leaf_count(ast, *right),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => leaf_count(ast, *then_branch) + leaf_count(ast, *else_branch),
        Expr::As { expr, .. } => leaf_count(ast, *expr),
        _ => 1,
    }
}

/// A function body, or a tagged template whose template object is
/// cached per syntax node (§13.2.8.4): a copy in the second branch
/// would be a second site with an object of its own.
pub(super) fn holds_site(ast: &Ast, e: ExprId) -> bool {
    matches!(ast.get_expr(e), Expr::ArrowFn { .. } | Expr::Closure { .. })
        || crate::ssa_lower_call_template_object::is_template_object_call(ast, e)
        || expr_children(ast, e)
            .into_iter()
            .any(|c| holds_site(ast, c))
}

/// The spelled nodes above the leaves no longer belong to the tree;
/// left in the arena they would still show a by-arena scan the leaf
/// as their operand.
pub(super) fn neutralize(ast: &mut Ast, e: ExprId) {
    let kids: Vec<ExprId> = match ast.get_expr(e) {
        Expr::Sequence { right, .. } => vec![*right],
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => vec![*then_branch, *else_branch],
        Expr::As { expr, .. } if spelled(ast, *expr) => vec![*expr],
        _ => return,
    };
    ast.exprs[e.0 as usize] = Expr::Null;
    for k in kids {
        neutralize(ast, k);
    }
}
