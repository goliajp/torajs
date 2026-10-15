//! The callee shapes the spelled-callee rewrite reads: which
//! operands lead down to the callee, and what sits at their leaves.

use super::receiver_names::Receivers;
use crate::ast::desugar_with::walk::expr_children;
use crate::ast::{Ast, Expr, ExprId};

/// The operands one step down that can produce the callee value: the
/// last operand of a comma, both arms of a conditional, both sides of
/// `??` whose left side is a name (distributed, the name is read once
/// for the test and once as the callee, which a name read allows), and
/// the operand of a cast.
pub(super) fn operands(ast: &Ast, e: ExprId) -> Option<Vec<ExprId>> {
    match ast.get_expr(e) {
        Expr::Sequence { right, .. } => Some(vec![*right]),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => Some(vec![*then_branch, *else_branch]),
        Expr::Nullish { lhs, rhs } if nullish_name(ast, *lhs).is_some() => Some(vec![*lhs, *rhs]),
        Expr::As { expr, .. } => Some(vec![*expr]),
        _ => None,
    }
}

/// The name on the left of a distributable `??`, under any casts.
pub(super) fn nullish_name(ast: &Ast, mut e: ExprId) -> Option<String> {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    match ast.get_expr(e) {
        Expr::Ident(n) if n != "eval" => Some(n.clone()),
        _ => None,
    }
}

/// A comma, conditional or `??` on the way down to the callee.
pub(super) fn spelled(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::As { expr, .. } => spelled(ast, *expr),
        _ => operands(ast, e).is_some(),
    }
}

pub(super) fn leaves_are_names(ast: &Ast, e: ExprId) -> bool {
    match operands(ast, e) {
        Some(v) => v.into_iter().all(|k| leaves_are_names(ast, k)),
        None => matches!(ast.get_expr(e), Expr::Ident(n) if n != "eval"),
    }
}

pub(super) fn names_a_leaf(ast: &Ast, e: ExprId, wanted: &Receivers) -> bool {
    match operands(ast, e) {
        Some(v) => v.into_iter().any(|k| names_a_leaf(ast, k, wanted)),
        None => wanted.names(ast, e),
    }
}

pub(super) fn leaf_count(ast: &Ast, e: ExprId) -> usize {
    match operands(ast, e) {
        Some(v) => v.into_iter().map(|k| leaf_count(ast, k)).sum(),
        None => 1,
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
    if !spelled(ast, e) {
        return;
    }
    let kids = operands(ast, e).unwrap_or_default();
    ast.exprs[e.0 as usize] = Expr::Null;
    for k in kids {
        neutralize(ast, k);
    }
}
