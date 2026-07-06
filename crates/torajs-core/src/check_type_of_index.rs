//! `Expr::Index` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Index` arm
//! as chunk-312 of the type_of_inner decomp.
//!
//! Rules:
//! - Index expression `index` must typecheck to `Type::Number`
//!   (V3-18 index-expression spec narrow; non-number index is a
//!   typecheck error rather than a silent runtime coercion) —
//!   except a `Type::Any` receiver, which also admits string keys
//!   (L3b #13; ES ToPropertyKey).
//! - `Type::String` indexed → `Type::String` (1-char substring;
//!   spec §6.1.4 string-indexed access returns a length-1 string).
//! - `Type::Array(T)` indexed → element type `T`.
//! - `Type::Any` indexed → `Type::Any` (TS `any` admits every
//!   member/index access; runtime dispatch via
//!   `__torajs_any_index_get` — Any-dynamic-access RFC 20260704 S3).
//! - Any other receiver type → error.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    index: ExprId,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, obj)?;
    let idx_ty = checker.type_of(ast, index)?;
    // L3b #13 — an `any` receiver admits string keys (ES
    // ToPropertyKey: `o["k"]` ≡ `o.k`); every other receiver keeps
    // the number-only spec narrow.
    if idx_ty != Type::Number && !(matches!(obj_ty, Type::Any) && idx_ty == Type::String) {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    match obj_ty {
        Type::String => Ok(Type::String),
        Type::Array(elem) => Ok(*elem),
        // RC-4 F1a — un-narrowed Nullable<Array<T>> (exec/match
        // result) decays for indexing; null is a runtime
        // TypeError at the lowering-side guard.
        Type::Nullable(inner) if matches!(*inner, Type::Array(_)) => match *inner {
            Type::Array(elem) => Ok(*elem),
            _ => unreachable!(),
        },
        Type::Any => Ok(Type::Any),
        other => Err(format!("can't index into {other:?}")),
    }
}
