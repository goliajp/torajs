//! A computed key evaluated ahead of a suspension: ToPropertyKey runs
//! right after the key's expression (§13.2.5.4 ComputedPropertyName),
//! so a key the lift hoists past a later await is converted where it
//! is hoisted, not when the object or class is made.

use super::{LiftLog, wrap};
use crate::ast::desugar_async_sm::expr_has_await;
use crate::ast::{Ast, BinOp, Expr, ExprId};

/// Wrap every key among `kids` (in evaluation order) that is evaluated
/// before the last child that awaits in `__torajs_class_computed_key`,
/// unless its value is a primitive whatever it evaluates to.
pub(in crate::ast) fn convert_keys_ahead(
    ast: &mut Ast,
    kids: &[ExprId],
    is_key: impl Fn(ExprId) -> bool,
    log: &mut LiftLog,
) {
    let Some(last) = kids.iter().rposition(|k| expr_has_await(ast, *k)) else {
        return;
    };
    for k in &kids[..last] {
        if is_key(*k) && !primitive(ast, *k) {
            let to_key = |ast: &mut Ast, moved| {
                let callee = ast.add_expr(Expr::Ident("__torajs_class_computed_key".into()));
                Expr::Call {
                    callee,
                    args: vec![moved],
                }
            };
            wrap(ast, *k, to_key, log);
        }
    }
}

/// The key's value is a primitive whatever it evaluates to, so its
/// ToPropertyKey runs no user code and may come later.
fn primitive(ast: &Ast, k: ExprId) -> bool {
    match ast.get_expr(k) {
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Unary { .. }
        | Expr::TypeOf { .. }
        | Expr::Delete { .. }
        | Expr::InstanceOf { .. }
        | Expr::PostIncr { .. } => true,
        Expr::BinOp { op, .. } => !matches!(op, BinOp::LAnd | BinOp::LOr),
        Expr::Ident(n) => n == "undefined",
        _ => false,
    }
}
