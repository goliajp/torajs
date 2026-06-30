//! `Array<T>.{reduce,reduceRight}(fn, init, ...trailing)`
//! Array-receiver trailing-arg ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 295 — eighty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §22.1.3.{21,22}: `Array.prototype.reduce` /
//! `Array.prototype.reduceRight` reserve slots past the 2
//! useful args (callback + initialValue) but tora's inline
//! reduce loop is 2-arg only. Trailing operand type_of'd for
//! side effects then dropped at lower-time; SSA-emit reads
//! only args[0..=1] so args[2..] are silently ignored
//! without any SSA-side change. Same shape as S243/S244 in
//! the original S245/S276 wedge (`== 3` widened to `>= 3`
//! per ES §23.1.3.{22,23} trailing-arg ignore).
//!
//! Returns:
//! - `Some(Ok(inner))` — elem type of the receiver Array
//! - `Some(Err(_))` on receiver `type_of` failure, arg 0
//!   callback type violation (`Array.reduce arg 0 must be
//!   a callback function, got T`), arg 1 init type mismatch
//!   (`Array.reduce arg 1 (initial) must match elem type T,
//!   got U`), or any trailing arg `type_of` failure
//! - `None` otherwise (non-Member callee, m_name outside
//!   {reduce, reduceRight}, args.len() < 3, or receiver not
//!   Array — cascade falls through)

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
    if !matches!(m_name.as_str(), "reduce" | "reduceRight") {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let inner = (**elem).clone();
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let fn_ok = matches!(aty0, Type::Function(..) | Type::Any);
    if !fn_ok {
        return Some(Err(format!(
            "Array.{m_name} arg 0 must be a callback function, got {aty0:?}"
        )));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if aty1 != inner && !matches!(inner, Type::Any) {
        return Some(Err(format!(
            "Array.{m_name} arg 1 (initial) must match elem type {:?}, got {aty1:?}",
            inner
        )));
    }
    for &a in &args[2..] {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(inner))
}
