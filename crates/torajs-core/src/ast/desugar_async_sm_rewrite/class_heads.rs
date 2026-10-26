//! The class arm of the await rewrite: a class declaration evaluates
//! its `extends` operand and its computed keys where it stands
//! (§15.7.14 ClassDefinitionEvaluation), so their awaits lift ahead of
//! it like an expression statement's operands, in that order.

use super::super::desugar_async_sm::expr_has_await;
use super::super::desugar_async_ssa_gate::class_exprs;
use super::super::{Ast, Stmt};
use super::{Cx, rewrite_list, with_prefix};
use crate::ast::desugar_async_sm_lift::{LiftLog, can_lift, convert_keys_ahead, lift_ordered};

/// The heads of the class `s`, lifted. An `extends` operand that
/// awaits is not a bare class name, which the class lowering does not
/// take yet; it is refused here, and the lowering names it.
pub(super) fn rewrite_class(ast: &mut Ast, s: &mut Stmt, cx: &Cx, log: &mut LiftLog) -> bool {
    let Stmt::ClassDecl { parent, .. } = &*s else {
        unreachable!("rewrite_class on a class declaration")
    };
    let parent = *parent;
    if parent.is_some_and(|p| expr_has_await(ast, p)) {
        return false;
    }
    let keys: Vec<_> = class_exprs(ast, s)
        .into_iter()
        .filter(|k| Some(*k) != parent)
        .collect();
    if !keys.iter().any(|k| expr_has_await(ast, *k)) {
        return true;
    }
    if !keys.iter().all(|k| can_lift(ast, *k)) {
        return false;
    }
    convert_keys_ahead(ast, &keys, |_| true, log);
    let mut prefix = Vec::new();
    lift_ordered(ast, &keys, &mut prefix, &cx.stable, log, cx.mark);
    if !rewrite_list(ast, &mut prefix, cx, log) {
        return false;
    }
    with_prefix(s, prefix);
    true
}
