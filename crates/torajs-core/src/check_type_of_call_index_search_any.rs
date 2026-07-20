//! `xs.{indexOf,lastIndexOf,includes}(anyNeedle, from?)` Any-needle
//! admit over a typed-element Array receiver (rotation 159 — the
//! index-search lane of the rotation-158 admit/coerce pairing,
//! sister to [`crate::check_type_of_call_array_push_unshift`]'s
//! 1-arg Any arm).
//!
//! TS any-assignability admits the needle; the lowering side pairs
//! it with `emit_compare`'s needle-Any arm (the ELEMENT packs as a
//! `(tag, value)` pair against the boxed needle — strict equality
//! is by-tag per §7.2.15, never a ToNumber coercion, and `includes`
//! rides the SameValueZero kernel per §23.1.3.16). An admit without
//! that pairing would compare box bits against raw slots and answer
//! a silent constant miss.
//!
//! Gated to Number / String elements (the push/fill lane bar).
//! Returns `Some(Ok(Number))` for indexOf / lastIndexOf,
//! `Some(Ok(Boolean))` for includes; `Some(Err(_))` on a bad
//! fromIndex type; `None` otherwise (falls to the member-table
//! path).

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
    if !matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
        || !(1..=2).contains(&args.len())
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    if !matches!(*elem, Type::Number | Type::String) {
        return None;
    }
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if needle_ty != Type::Any {
        return None;
    }
    if args.len() == 2 {
        let from_ty = match checker.type_of(ast, args[1]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(from_ty, Type::Number | Type::Undefined | Type::Any) {
            return Some(Err(format!(
                "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
            )));
        }
    }
    Some(Ok(if m_name == "includes" {
        Type::Boolean
    } else {
        Type::Number
    }))
}
