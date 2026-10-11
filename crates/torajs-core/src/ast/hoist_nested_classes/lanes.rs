//! The nested-class hoist's capturing lane: the name sets its capture
//! check resolves against, and the in-place rewrites that record the
//! frames around what they mint — split out of the parent under the
//! file-size line.

use crate::ast::scope_chain::list_names;
use crate::ast::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// What the capture check resolves a free name against.
pub(in crate::ast) struct HoistNames {
    /// Top-level visible names — the prebound set for the capture
    /// check plus the collision domain for renaming.
    pub(in crate::ast) top: Vec<String>,
    /// What the frames between the container being walked and the top
    /// level bind (`scope_chain`): a top-level name in here is
    /// rebound, and a class reading it captures.
    pub(in crate::ast) shadow: Vec<String>,
    /// Function expressions the capturing lane minted while the walk
    /// ran, with the frames around them — the arena scan reaches
    /// them after the chains were computed.
    pub(super) minted: HashMap<ExprId, Vec<String>>,
}

/// The capturing lane, recording the frames around each function
/// expression it mints for the arena scan that reaches them later.
pub(super) fn rewrite_capturing(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    counter: &mut u32,
    names: &mut HoistNames,
) -> bool {
    let before = ast.exprs.len();
    let taken =
        crate::ast::capturing_classes::try_rewrite_capturing_class(ast, stmts, idx, counter);
    let mut around = names.shadow.clone();
    list_names(stmts, &mut around);
    for i in before..ast.exprs.len() {
        if matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            names.minted.insert(ExprId(i as u32), around.clone());
        }
    }
    taken
}

/// A unit-shaped class the unit lane refused (a sibling body names
/// it, say) still carries observable static work, and the shared
/// hoist would run that at the program's end instead of where the
/// class is written. When no class this level already `lifted` names
/// it (those bodies now run at the top level, where a binding kept in
/// the container is out of reach) and the capturing lane will take
/// it, that lane rewrites it in place and answers `true` — a later
/// sibling that reads it then sees a local and takes the same lane,
/// so the two evaluate in source order.
pub(super) fn in_place_for_statics(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    counter: &mut u32,
    (lifted, name): (&[Stmt], &str),
    names: &mut HoistNames,
) -> bool {
    let mut occ = crate::ast::class_unit_census::Occurrences::default();
    for s in lifted
        .iter()
        .filter(|s| matches!(s, Stmt::ClassDecl { .. }))
    {
        occ.stmt(ast, s, name);
    }
    occ.name == 0
        && crate::ast::capturing_classes::decline::would_claim(ast, &stmts[idx])
        && rewrite_capturing(ast, stmts, idx, counter, names)
}
