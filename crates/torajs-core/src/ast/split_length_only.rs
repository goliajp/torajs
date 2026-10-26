//! Which `s.split(sep)` calls only ever have their product measured —
//! the census the split-count lane consults (rotation 672).
//!
//! `s.split(sep)` builds an array of views, and a program that then
//! reads nothing but its `.length` pays for every cell, its release
//! and the pool round-trip to learn one number. The array is never
//! observable in these shapes — no element is read, its identity is
//! never compared, nothing is written into it — so the lowering
//! replaces it by the number (scalar replacement of an aggregate that
//! does not escape): `__torajs_str_split_count`, which counts the
//! tokens without building them.
//!
//! Two shapes qualify:
//!
//! - `R.split(S).length` — the product is a temporary whose one use is
//!   the length read.
//! - `let` / `const` `x = R.split(S)` where every use of `x` in its
//!   scope is `x.length` in a read position. A write (`x.length = n`,
//!   `x.length++`, `delete x.length`), a call through it
//!   (`x.length()`), any other member, index or bare use, a lifted
//!   closure that captured `x`, an arrow, or a function or class
//!   declaration anywhere in the scope disqualifies it: a shape the
//!   walk can not prove is a length read counts as a use.
//!
//! Only a one-argument call qualifies (a limit slices the product).
//! Whether the receiver and separator are strings is not known here;
//! the lowering takes the lane only when both are, so an entry for
//! any other call is inert.
//!
//! ## Shape
//!
//! Keyed by the separator argument's ExprId — the one handle
//! `ssa_lower_str_str_split::lower_split` holds for its call site. The
//! walk, the reference counts and the scope rules are the shared ones
//! in [`super::scalar_census`].

use std::collections::HashSet;

use super::scalar_census::{Census, Lane, census_bindings, is_x};
use super::{Ast, Expr, ExprId};

/// The separator ExprIds of the one-argument split calls whose product
/// is only ever measured.
pub(super) fn split_length_only_seps(ast: &Ast, census: &Census) -> HashSet<ExprId> {
    let mut found: HashSet<ExprId> = HashSet::new();
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        if let Expr::Member { obj, name } = ast.get_expr(eid)
            && name == "length"
            && !census.not_read.contains(&eid)
            && let Some(sep) = one_arg_split_sep(ast, *obj)
        {
            census.accept(*obj, sep, &mut found);
        }
    }
    found.extend(census_bindings(ast, &SplitLane, census));
    found
}

/// `R.split(S)` with exactly one non-spread argument: `S`.
fn one_arg_split_sep(ast: &Ast, call: ExprId) -> Option<ExprId> {
    let Expr::Call { callee, args } = ast.get_expr(call) else {
        return None;
    };
    let Expr::Member { name, .. } = ast.get_expr(*callee) else {
        return None;
    };
    match args.as_slice() {
        [sep] if name == "split" && !matches!(ast.get_expr(*sep), Expr::Spread { .. }) => {
            Some(*sep)
        }
        _ => None,
    }
}

/// A bound split product's one use: `x.length` read as a value.
struct SplitLane;

impl Lane for SplitLane {
    fn produced(&self, ast: &Ast, init: ExprId) -> Option<(ExprId, ExprId)> {
        one_arg_split_sep(ast, init).map(|sep| (init, sep))
    }

    fn leaf(&self, ast: &Ast, e: ExprId, x: &str, _: &Census) -> Option<bool> {
        match ast.get_expr(e) {
            Expr::Member { obj, name } if name == "length" && is_x(ast, *obj, x) => Some(true),
            _ => None,
        }
    }
}
