//! `Array<T>.{reduce,reduceRight}(cb, init?)` accumulator check —
//! the two TypeScript overloads of the reducer, enforced before any
//! route claims the call:
//!
//! - with `init`: `reduce<U>(cb: (prev: U, cur: T, …) => U, init: U)`
//!   — the accumulator parameter must take `init`, and the callback's
//!   return must fit back into the accumulator parameter;
//! - without: `reduce(cb: (prev: T, cur: T, …) => T)` — the first
//!   accumulator is the first element, so the parameter must take `T`
//!   and the return must fit it.
//!
//! In both, the element parameter must take `T`. A program that fails
//! one of these has no runtime meaning of its own to preserve (a TS
//! type error), and the lanes downstream type the accumulator slot
//! from the parameter: a `(acc: string, x) => acc + x` fed `0` would
//! convert the seed at the slot boundary and print `0123` where the
//! JS program prints `6`. Only pairs that can never share a value are
//! refused — two distinct primitives, or a primitive against an array
//! or a function — so `any` on either side, unions, and object or
//! array accumulators seeded with `{}` / `[]` pass untouched.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Result<(), String> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !matches!(name.as_str(), "reduce" | "reduceRight") || args.is_empty() {
        return Ok(());
    }
    let Ok(Type::Array(elem)) = crate::check::receiver_type(checker, ast, *obj) else {
        return Ok(());
    };
    let Type::Function(ps, ret) = checker.type_of(ast, args[0])? else {
        return Ok(());
    };
    if let Some(cur) = ps.get(1)
        && disjoint(cur, &elem)
    {
        return Err(format!(
            "Array.{name} callback element parameter is {cur:?} but the array holds {elem:?}"
        ));
    }
    let Some(acc) = ps.first() else {
        return Ok(());
    };
    match args.get(1) {
        Some(&init) => {
            let init_ty = checker.type_of(ast, init)?;
            if disjoint(acc, &init_ty) {
                return Err(format!(
                    "Array.{name} callback accumulator parameter is {acc:?} but the initial value is {init_ty:?}"
                ));
            }
        }
        None if disjoint(acc, &elem) => {
            return Err(format!(
                "Array.{name} callback accumulator parameter is {acc:?} but with no initial value the first accumulator is an element of type {elem:?}"
            ));
        }
        None => {}
    }
    if disjoint(acc, &ret) {
        return Err(format!(
            "Array.{name} callback returns {ret:?} but its accumulator parameter is {acc:?}"
        ));
    }
    Ok(())
}

/// No value has both types.
fn disjoint(a: &Type, b: &Type) -> bool {
    let primitive = |t: &Type| {
        matches!(
            t,
            Type::Number | Type::String | Type::Boolean | Type::BigInt
        )
    };
    let structured = |t: &Type| matches!(t, Type::Array(_) | Type::Function(..));
    (primitive(a) && primitive(b) && a != b)
        || (primitive(a) && structured(b))
        || (structured(a) && primitive(b))
}
