//! `d.{getTime, valueOf, toISOString, toJSON, getFullYear,
//! getUTCFullYear, getMonth, getUTCMonth, getDate,
//! getUTCDate, getHours, getUTCHours, getMinutes,
//! getUTCMinutes, getSeconds, getUTCSeconds,
//! getMilliseconds, getUTCMilliseconds, getDay, getUTCDay,
//! getTimezoneOffset, getYear, toGMTString, toUTCString,
//! toDateString, toLocaleString, toLocaleDateString,
//! toLocaleTimeString}(...trailing)` Date-instance 0-arg
//! getter / format method family trailing-arg ignore wedge
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 288 — eightieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S261 — Date instance 0-arg getter / format method family
//! trailing-arg ignore per ES §21.4.4.*. Spec methods either
//! take 0 args or have optional `key` / `locales` / `options`
//! args that tora's subset silently drops; the SSA-emit
//! default branch (ssa_lower.rs ~21601 `else { vec![recv_op] }`)
//! only forwards `recv_op` for these 0-arg methods, so
//! trailing user args naturally drop. Narrow widen: matches
//! the 0-arg method list, `Type::Date` receiver, args.len()
//! >= 1.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Date` + m_name ∈ {Date 0-arg getter/format
//!   family above} + args.len() >= 1:
//!     - typecheck-drop all args
//!     - returns:
//!       - `Some(Ok(Type::String))` for format methods
//!         (`toISOString`, `toJSON`, `toGMTString`,
//!         `toUTCString`, `toDateString`, `toLocaleString`,
//!         `toLocaleDateString`, `toLocaleTimeString`)
//!       - `Some(Ok(Type::Number))` for the rest (epoch /
//!         component getters)
//!
//! Returns:
//! - `Some(Ok(_))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   the family, args empty, or non-Date receiver — cascade
//!   falls through to the S260 Number.toLocaleString sibling
//!   and beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(
        m_name.as_str(),
        "getTime"
            | "valueOf"
            | "toISOString"
            | "toJSON"
            | "getFullYear"
            | "getUTCFullYear"
            | "getMonth"
            | "getUTCMonth"
            | "getDate"
            | "getUTCDate"
            | "getHours"
            | "getUTCHours"
            | "getMinutes"
            | "getUTCMinutes"
            | "getSeconds"
            | "getUTCSeconds"
            | "getMilliseconds"
            | "getUTCMilliseconds"
            | "getDay"
            | "getUTCDay"
            | "getTimezoneOffset"
            | "getYear"
            | "toGMTString"
            | "toUTCString"
            | "toDateString"
            | "toLocaleString"
            | "toLocaleDateString"
            | "toLocaleTimeString"
    ) {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let recv_ty = match checker.type_of(ast, *recv_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(recv_ty, Type::Date) {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(
        if matches!(
            m_name.as_str(),
            "toISOString"
                | "toJSON"
                | "toGMTString"
                | "toUTCString"
                | "toDateString"
                | "toLocaleString"
                | "toLocaleDateString"
                | "toLocaleTimeString"
        ) {
            Type::String
        } else {
            Type::Number
        },
    ))
}
