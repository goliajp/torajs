//! The top-level names a frame around a site rebinds — the ones a
//! pass that pre-binds the top level must not pre-bind there.

use super::chains_by_arrow;
use crate::ast::{Ast, ExprId};
use std::collections::HashMap;

/// `globals` less the names in `shadow`.
pub(in crate::ast) fn not_rebound(globals: &[String], shadow: &[String]) -> Vec<String> {
    globals
        .iter()
        .filter(|g| !shadow.contains(g))
        .cloned()
        .collect()
}

/// The subset of `globals` the frames around each function expression
/// bind — the names `lift_arrow_fns` must not pre-bind for it.
pub(in crate::ast) fn shadowed_globals_by_arrow(
    ast: &Ast,
    globals: &[String],
) -> HashMap<ExprId, Vec<String>> {
    let mut out = chains_by_arrow(ast, &[&ast.stmts]);
    out.retain(|_, chain| {
        let mut hit: Vec<String> = globals
            .iter()
            .filter(|g| chain.contains(g))
            .cloned()
            .collect();
        std::mem::swap(chain, &mut hit);
        !chain.is_empty()
    });
    out
}
