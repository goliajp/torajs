//! `s.{at,charAt,charCodeAt,codePointAt,repeat,normalize}(
//! useful, ...trailing)` trailing-arg ignore arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 246 — thirty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S240 — per ES §22.1.3.{1,2,3,4,17,13}: spec reserves
//! slots past useful arg 0 but tora's helpers are 1-arg
//! only. Trailing operands type_of'd for side effects;
//! ssa_lower mirror evals-and-drops past i=0.
//!
//! S272 widens from `args.len() == 2` (single trailing)
//! to `args.len() >= 2` (any trailing count).
//!
//! arg0 type-gate: `at` / `charAt` / `charCodeAt` /
//! `codePointAt` / `repeat` accept `Number | Undefined`;
//! `normalize` accepts `String | Undefined`.
//!
//! Returns `Type::String` for `at` / `charAt` / `repeat` /
//! `normalize`, `Type::Number` for `charCodeAt` /
//! `codePointAt`, only when the receiver is `Type::String`,
//! arg0 type-ok, and `args.len() >= 2`. `Some(Err(_))` on
//! recursive `type_of` failure. `None` otherwise (cascade
//! falls through to the strict method table).

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
    if !matches!(
        m_name.as_str(),
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "normalize"
    ) || args.len() < 2
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let arg0_ok = match m_name.as_str() {
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" => {
            matches!(aty0, Type::Number | Type::Undefined)
        }
        "normalize" => matches!(aty0, Type::String | Type::Undefined),
        _ => false,
    };
    if !arg0_ok {
        return None;
    }
    for &a in &args[1..] {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "at" | "charAt" | "repeat" | "normalize" => Type::String,
        _ => Type::Number,
    }))
}
