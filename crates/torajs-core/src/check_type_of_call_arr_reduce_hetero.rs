//! `Array<T>.reduce(cb, seed)` / `.reduceRight(cb, seed)`
//! heterogeneous accumulator wedge — the method-table entry
//! declares the shape as `((T, T) => T, T) => T` and rejects
//! e.g. `[1,2,3].reduce((a: string, x: number) => a + x, '')`
//! even though it is legal TypeScript (spec §23.1.3.24 lets
//! the accumulator U differ from the element T when a seed of
//! type U is supplied).
//!
//! Accepts:
//! - cb `(U, T) => U` — accumulator = U, element = T
//! - cb `(any, T) => R` — Any-param widening (contravariance);
//!   result follows the cb's actual return
//! for U ∈ {Number, String, Boolean, Any}. Homogeneous
//! `(T, T) => T` stays with the method-table arm; a Void-ret
//! reduce keeps the sister [`arr_pred_void_cb`] Any-fold lane.

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
    if m_name != "reduce" && m_name != "reduceRight" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Function(ps, ret) = &cb_ty else {
        return None;
    };
    if ps.len() < 1 || ps.len() > 4 {
        return None;
    }
    // Second cb param (when present) must be elem-typed or Any —
    // gate the receiver flow through as-is (contravariance).
    if ps.len() >= 2 && ps[1] != **elem && ps[1] != Type::Any && **elem != Type::Any {
        return None;
    }
    // First cb param (the accumulator) types the result. Same-`T`
    // acc keeps the method-table arm; only primitive-U lanes we
    // can round-trip through the run-time boxed acc slot are
    // admitted here.
    let acc_ty = &ps[0];
    if acc_ty == elem.as_ref() {
        return None;
    }
    if !matches!(
        acc_ty,
        Type::Number | Type::String | Type::Boolean | Type::Any
    ) {
        return None;
    }
    // Void ret is the sister wedge's territory.
    if matches!(**ret, Type::Void) {
        return None;
    }
    // The seed (args[1]) should agree with acc — the runtime box
    // stores it as the acc slot's shape and a mismatch would
    // silent-mistype the very first fold.
    if let Some(&seed_id) = args.get(1) {
        let seed_ty = match checker.type_of(ast, seed_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if seed_ty != *acc_ty && !matches!(*acc_ty, Type::Any) && !matches!(seed_ty, Type::Any) {
            return Some(Err(format!(
                "Array<{elem:?}>.{m_name} seed is {seed_ty:?} but accumulator is {acc_ty:?}"
            )));
        }
    }
    // type_of + drop remaining args (thisArg lane).
    for &t in args.iter().skip(2) {
        if let Err(e) = checker.type_of(ast, t) {
            return Some(Err(e));
        }
    }
    Some(Ok((**ret).clone()))
}
