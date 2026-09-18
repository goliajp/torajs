//! The hoist's per-evaluation lane (RFC 20260917-class-eval-unit):
//! whether a shape-admitted class at `stmts[idx]` becomes a unit class,
//! and what that leaves where it was written. Split out of
//! `walk_container` (verbatim move), which answers which LANE a nested
//! class takes; this file answers what the unit lane does with it.

use super::{Ast, Stmt};

/// Turn `stmts[idx]` into a unit class when every reference to
/// `binding` stays in this container: the class and its `__cunit_`
/// factory join `hoisted`, the residual `let` takes the class's place.
/// `false` leaves everything untouched for the shared-identity hoist.
pub(super) fn take_unit_lane(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    binding: &str,
    hoisted: &mut Vec<Stmt>,
) -> bool {
    if !super::class_unit_admit::refs_stay_local(ast, stmts, idx, binding) {
        return false;
    }
    let mut cls = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    let (slot, unit_fn) = super::class_unit::make_unit(ast, &mut cls, binding);
    stmts[idx] = slot;
    hoisted.push(cls);
    hoisted.push(unit_fn);
    true
}
