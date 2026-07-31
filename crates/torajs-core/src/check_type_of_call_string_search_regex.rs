//! `String.search(RegExp)` wedge arm (chunk 800) — the recorded
//! follow-up in [`crate::check_type_of_member_string`]'s search
//! entry ("RegExp arg is a follow-up substrate item"): the method
//! table declares `(String) -> Number` for the plain-string needle,
//! so a RegExp arg fell through to the general arm and rejected
//! loud with "argument 0: expected String, got RegExp" (bun answers
//! the match index per ES §22.1.3.19).
//!
//! Receiver / m_name / arg matrix:
//! - `Type::String` receiver + m_name == "search" + args[0] typed
//!   `Type::RegExp` → `Some(Ok(Type::Number))`. Trailing args are
//!   type_of'd for error propagation then ignored (ES reads only
//!   the first arg; the SSA side lowers them for side effects per
//!   the S286 family idiom).
//! - anything else → `None` (cascade falls through to the S210
//!   short-circuit / S324 trailing / general method-table arms).
//!
//! Lowering lands in `ssa_lower_call_str_regex_methods` (the
//! regex-arg intercept station shared with match / matchAll /
//! split / replace), which routes to `__torajs_str_search_regex`.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "search" || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let a0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // Rotation 262 — §22.1.3.20 single-arg Any pattern: with store
    // evidence the custom `@@search` may run and return anything
    // (`any`); without it the step-4 RegExpCreate coerce lane
    // answers a Number. Mirrors `check_type_of_call_string_match`'s
    // gate; the SSA twin branches on the same shared fn.
    if args.len() == 1 && matches!(a0, Type::Any) {
        if crate::check_type_of_call_string_match::any_pattern_may_carry_matcher(ast, args[0]) {
            return Some(Ok(Type::Any));
        }
        return Some(Ok(Type::Number));
    }
    if !matches!(a0, Type::RegExp) {
        return None;
    }
    for &aid in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Number))
}
