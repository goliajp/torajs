//! What one row's default becomes when its slot is widened.
//!
//! The parent decides what shape a slot settles on; this decides
//! whether a row that owns a default can survive that shape. The two
//! are separate questions because the second one has a veto: a
//! default that cannot be evaluated in the callee keeps the slot
//! refused no matter how well the annotations join.
//!
//! A default on a class method is supplied by the CALL SITE
//! (`apply_default_args` pads by method name, and only when every
//! owner of that name agrees), so widening a slot is exactly what
//! makes those owners disagree — the row that owns the default would
//! stop receiving it. Moving it into the body is the way out, and the
//! gate on that move is the one plain functions already use.

use std::collections::HashMap;

use super::super::super::{Ast, Expr, ExprId, Param, Stmt};

/// What becomes of one row's default once its slot is widened.
pub(super) enum DefaultFate {
    /// Nothing to carry: no default, or the `undefined` literal that
    /// is already what a pad sends.
    Absent,
    /// Movable into the body, guarded on the parameter itself.
    Move(ExprId),
    /// Cannot move — the slot keeps today's refusal.
    Blocking,
}

/// Read one parameter's default against the body-guard channel —
/// `apply_args_materialize::guardable_default`, the same gate that
/// decides whether a plain function's default may be evaluated in the
/// callee. It answers `None` both for a default nothing needs to
/// carry and for one nothing may, so the two cases the slot cares
/// about are separated here: an absent default and this pass's own
/// pad value leave the row alone, anything else it declines to move
/// keeps the slot refused (`__yield_arg` is the one named exclusion —
/// the generator resume slot keeps one shape-uniform default across
/// every lane, and converting a single copy evicts `next` from the
/// pad table for all of them).
fn default_fate(ast: &Ast, p: &Param, prior: &[String], global_fns: &[String]) -> DefaultFate {
    let Some(d) = p.default else {
        return DefaultFate::Absent;
    };
    if matches!(ast.get_expr(d), Expr::Ident(n) if n == "undefined") {
        return DefaultFate::Absent;
    }
    match super::super::super::apply_args_materialize::guardable_default(ast, p, global_fns, prior)
    {
        Some(d) => DefaultFate::Move(d),
        None => DefaultFate::Blocking,
    }
}

/// Every row's default, read in parameter order — `None` when some
/// row carries one that may not move.
pub(super) fn root_default_fates(
    ast: &Ast,
    rows: &[String],
    sigs: &HashMap<String, Vec<Param>>,
    global_fns: &[String],
) -> Option<Vec<Vec<DefaultFate>>> {
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let ps = &sigs[r];
        let mut fates = Vec::with_capacity(ps.len());
        for (pi, p) in ps.iter().enumerate() {
            let prior: Vec<String> = ps[..pi].iter().map(|q| q.name.clone()).collect();
            match default_fate(ast, p, &prior, global_fns) {
                DefaultFate::Blocking => return None,
                f => fates.push(f),
            }
        }
        out.push(fates);
    }
    Some(out)
}

/// Top-level function names, for the arrow-shaped default's free-var
/// test (`body_safe_default`'s only use of them).
pub(super) fn toplevel_fn_names(ast: &Ast) -> Vec<String> {
    super::super::super::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}
