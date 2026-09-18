//! The hoist's per-evaluation lane (RFC 20260917-class-eval-unit):
//! whether a shape-admitted class at `stmts[idx]` becomes a unit class,
//! and what that leaves where it was written. Split out of
//! `walk_container` (verbatim move), which answers which LANE a nested
//! class takes; this file answers what the unit lane does with it.

use super::{Ast, Expr, Stmt};

/// K5-1 — a unit class and the siblings that extend it are one family:
/// a derived class that kept the shared identity would chain its
/// template prototype under the TEMPLATE parent, while `instanceof`
/// asks this evaluation's parent — `new B() instanceof A` answered
/// false. So the parent is decided with its children: every sibling
/// whose heritage names `binding` must itself be admissible (shape,
/// under the hypothesis that the parent is a unit class; references;
/// its own children), and a parent that has children may not declare
/// a STATIC private element yet (`admits_shape` holds the derived side
/// to the same; K5-2b). The derived classes come later in the list —
/// a class is in its dead zone before its own declaration — so this
/// looks ahead.
fn family_admits(ast: &Ast, stmts: &[Stmt], idx: usize, binding: &str, depth: u32) -> bool {
    let children: Vec<usize> = stmts
        .iter()
        .enumerate()
        .filter(|(j, s)| {
            *j != idx
                && matches!(s, Stmt::ClassDecl { parent: Some(pid), .. }
                    if matches!(ast.get_expr(*pid), Expr::Ident(p) if p == binding))
        })
        .map(|(j, _)| j)
        .collect();
    if children.is_empty() {
        return true;
    }
    if depth >= 64 || super::class_unit_admit::declares_static_private(&stmts[idx]) {
        return false;
    }
    children.into_iter().all(|j| {
        let Stmt::ClassDecl { name, .. } = &stmts[j] else {
            return false;
        };
        super::class_unit_admit::admits_shape(ast, &stmts[j], Some(binding))
            && super::class_unit_admit::refs_stay_local(ast, stmts, j, name)
            && family_admits(ast, stmts, j, name, depth + 1)
    })
}

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
    if !super::class_unit_admit::refs_stay_local(ast, stmts, idx, binding)
        || !family_admits(ast, stmts, idx, binding, 0)
    {
        return false;
    }
    let mut cls = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    let (slot, unit_fn) = super::class_unit::make_unit(ast, &mut cls, binding);
    stmts[idx] = slot;
    hoisted.push(cls);
    hoisted.push(unit_fn);
    true
}
