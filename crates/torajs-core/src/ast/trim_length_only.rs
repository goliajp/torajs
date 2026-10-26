//! Which `R.trim()` calls only ever have their product measured.
//!
//! `R.trim()` on a view answers a new view of the same parent, and a
//! program that then reads nothing but its `.length` pays for writing
//! that view, retaining the parent and releasing it again, to learn
//! one number: the distance between the two ends the scan stopped at.
//! The product is never observable in these shapes, so the lowering
//! answers the number instead (scalar replacement of an aggregate that
//! does not escape).
//!
//! Two shapes qualify:
//!
//! - `R.trim().length` — the product is a temporary whose one use is
//!   the length read.
//! - `let` / `const` `x = R.trim()` where every use of `x` in its scope
//!   is `x.length` in a read position, under the scope rules of
//!   [`super::scalar_census`].
//!
//! Only a call with no arguments qualifies. Whether the receiver is a
//! string view is not known here; the lowering takes the lane only when
//! it is, so an entry for any other call is inert.
//!
//! Keyed by the call's own ExprId, which the method dispatch holds.

use std::collections::HashSet;

use super::scalar_census::{Census, Lane, census_bindings, is_x};
use super::{Ast, Expr, ExprId};

/// The `R.trim()` calls whose product is only ever measured.
pub(super) fn trim_length_only_calls(ast: &Ast, census: &Census) -> HashSet<ExprId> {
    let mut found: HashSet<ExprId> = HashSet::new();
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        if let Expr::Member { obj, name } = ast.get_expr(eid)
            && name == "length"
            && !census.not_read.contains(&eid)
            && is_trim_call(ast, *obj)
        {
            census.accept(*obj, *obj, &mut found);
        }
    }
    found.extend(census_bindings(ast, &TrimLane, census));
    found
}

/// `R.trim()` with no arguments.
fn is_trim_call(ast: &Ast, call: ExprId) -> bool {
    let Expr::Call { callee, args } = ast.get_expr(call) else {
        return false;
    };
    args.is_empty() && matches!(ast.get_expr(*callee), Expr::Member { name, .. } if name == "trim")
}

/// A bound trim product's one use: `x.length` read as a value.
struct TrimLane;

impl Lane for TrimLane {
    fn produced(&self, ast: &Ast, init: ExprId) -> Option<(ExprId, ExprId)> {
        is_trim_call(ast, init).then_some((init, init))
    }

    fn leaf(&self, ast: &Ast, e: ExprId, x: &str, _: &Census) -> Option<bool> {
        match ast.get_expr(e) {
            Expr::Member { obj, name } if name == "length" && is_x(ast, *obj, x) => Some(true),
            _ => None,
        }
    }
}
