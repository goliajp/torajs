//! Which `const m = s.match(re)` results are only ever null-tested or
//! asked `m[0].length` — the census the match-length lane consults.
//!
//! A successful match builds an array and copies `m[0]` into a fresh
//! string, and a program that then only checks the result against
//! null and reads the first match's length pays for both, and for
//! their release, to learn one number. Neither is observable in these
//! shapes, so the lowering replaces the result by that number (scalar
//! replacement of an aggregate that does not escape):
//! `__torajs_str_match_len` answers the first match's UTF-16 length or
//! -1, with every lastIndex effect `match` has.
//!
//! A use of `m` is the lane's when it is
//!
//! - `m === null` / `!==` / `==` / `!=` against a bare `null` literal;
//! - `!m`, a bare `m` as an `if` / loop / `?:` condition;
//! - `m[0].length` read as a value.
//!
//! Anything else — `m[1]`, `m[0]` as a string, `m.length` (a match
//! count under `/g`), `m` handed anywhere, `m ?? x`, `m?.x`,
//! `typeof m`, a `null` hidden behind a cast — keeps the array.
//!
//! Only `s.match(re)` with one argument qualifies. Whether `s` is a
//! string and `re` a RegExp is not known here; the lowering takes the
//! lane only when both are (its `emit_match` is reached by nothing
//! else), so an entry for any other call is inert. Keyed by the
//! `.match` member ExprId, the handle `emit_match` holds. The walk and
//! the scope rules are the shared ones in [`super::scalar_census`].

use super::scalar_census::{Census, Lane, census_bindings, expr_uses_ok, is_x};
use super::{Ast, BinOp, Expr, ExprId, UnaryOp};

/// The `.match` member ExprIds whose result the lane may hold as a
/// length.
pub(super) fn regex_match_len_callees(
    ast: &Ast,
    census: &Census,
) -> std::collections::HashSet<ExprId> {
    census_bindings(ast, &MatchLane, census)
}

struct MatchLane;

impl Lane for MatchLane {
    fn produced(&self, ast: &Ast, init: ExprId) -> Option<(ExprId, ExprId)> {
        let Expr::Call { callee, args } = ast.get_expr(init) else {
            return None;
        };
        let Expr::Member { name, .. } = ast.get_expr(*callee) else {
            return None;
        };
        match args.as_slice() {
            [re] if name == "match" && !matches!(ast.get_expr(*re), Expr::Spread { .. }) => {
                Some((init, *callee))
            }
            _ => None,
        }
    }

    fn leaf(&self, ast: &Ast, e: ExprId, x: &str, census: &Census) -> Option<bool> {
        let is_null = |id: ExprId| matches!(ast.get_expr(id), Expr::Null);
        match ast.get_expr(e) {
            Expr::BinOp {
                op: BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq,
                left,
                right,
            } if (is_x(ast, *left, x) && is_null(*right))
                || (is_null(*left) && is_x(ast, *right, x)) =>
            {
                Some(true)
            }
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } if is_x(ast, *expr, x) => Some(true),
            Expr::Member { obj, name }
                if name == "length"
                    && !census.not_read.contains(&e)
                    && first_element_of_x(ast, *obj, x) =>
            {
                Some(true)
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } if is_x(ast, *cond, x) => Some(
                expr_uses_ok(ast, *then_branch, x, self, census)
                    && expr_uses_ok(ast, *else_branch, x, self, census),
            ),
            _ => None,
        }
    }

    fn bare_condition_ok(&self) -> bool {
        true
    }
}

/// `x[0]`.
fn first_element_of_x(ast: &Ast, e: ExprId, x: &str) -> bool {
    matches!(
        ast.get_expr(e),
        Expr::Index { obj, index }
            if is_x(ast, *obj, x) && matches!(ast.get_expr(*index), Expr::Number(n) if *n == 0.0)
    )
}
