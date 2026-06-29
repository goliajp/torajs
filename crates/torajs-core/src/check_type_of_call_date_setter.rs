//! Date instance setter (`d.set{FullYear,Month,Date,Hours,
//! Minutes,Seconds,Milliseconds,Time,Year}(..., ...trailing)`)
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 268 — sixty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S268 — Date instance setter trailing-arg ignore per
//! ES §21.4.4.{20-26}: each per-field setter accepts up
//! to N Number args (year/month/date/hours/etc); trailing
//! args beyond that silent-drop per ES trailing-arg
//! ignore. tora's fixed-N sigs rejected the next arg;
//! SSA-emit's `for i in 0..target_arity` loop already
//! takes only args[0..arity] so trailing operands fall
//! off the slice (release-build); pair with a skip(arity)
//! eval-and-drop loop in the matching widen below to
//! preserve side effects and replace the
//! `debug_assert!(args.len() <= arity)`.
//!
//! Per-method `max_arity`:
//! - `setFullYear` / `setMinutes` → 3
//! - `setMonth` / `setSeconds` → 2
//! - `setDate` / `setMilliseconds` / `setTime` / `setYear`
//!   → 1
//! - `setHours` → 4
//!
//! Arity gate: `args.len() > max_arity` (i.e. trailing-
//! widening — pure per-spec arities still hit the strict
//! method table below).
//!
//! Returns:
//! - `Some(Ok(Type::Number))` on Type::Date receiver + arity
//!   gate hit + all args type_of'd successfully
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   the setter allowlist, non-Date receiver, or arity not
//!   over max_arity — cascade falls through to the
//!   Array.isArray / general method dispatch arms below)

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
        "setFullYear"
            | "setMonth"
            | "setDate"
            | "setHours"
            | "setMinutes"
            | "setSeconds"
            | "setMilliseconds"
            | "setTime"
            | "setYear"
    ) {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Date) {
        return None;
    }
    let max_arity: usize = match m_name.as_str() {
        "setFullYear" | "setMinutes" => 3,
        "setMonth" | "setSeconds" => 2,
        "setDate" | "setMilliseconds" | "setTime" | "setYear" => 1,
        "setHours" => 4,
        _ => unreachable!(),
    };
    if args.len() <= max_arity {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Number))
}
