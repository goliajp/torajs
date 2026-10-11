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

/// The result type of a seeded reducer whose accumulator parameter
/// `acc` differs from the element type: TypeScript's second overload,
/// `reduce<U>(cb: (prev: U, cur: T, …) => U, init: U): U`, answers
/// the accumulator's `U`. The callback's return `ret` and the seed
/// (`args[1]`) must agree with `U`. For an object or array `U`, a
/// return the sniff could not name (`any`) is held to the annotation,
/// and so is an `any` seed: the lowering casts an accumulator that ran
/// on the any lane into `U`. A scalar `U` has no such cast, so an
/// `any` return or seed answers `any` there (the accumulator may be a
/// value of another kind, `acc + "a"` under `acc: number`). An empty
/// array literal seeds an array accumulator as written.
/// Anything else (no seed, an `any` or void accumulator, a return or
/// a seed of another type) answers `any`.
pub(crate) fn seeded_result(
    checker: &mut Checker,
    ast: &Ast,
    acc: &Type,
    ret: &Type,
    args: &[ExprId],
) -> Result<Type, String> {
    let Some(&seed) = args.get(1) else {
        return Ok(Type::Any);
    };
    let seed_ty = checker.type_of(ast, seed)?;
    let scalar = matches!(acc, Type::Number | Type::String | Type::Boolean);
    // only an object or array product crosses into `U` through the
    // lowering's checked cast; a scalar one stays a box
    let cast = matches!(acc, Type::Array(_) | Type::Struct(_));
    let ret_fits = ret == acc || (cast && *ret == Type::Any);
    let seed_fits = seed_ty == *acc
        || (cast && seed_ty == Type::Any)
        || (matches!(acc, Type::Array(_)) && empty_array_literal(ast, seed));
    Ok(if (scalar || cast) && ret_fits && seed_fits {
        acc.clone()
    } else {
        Type::Any
    })
}

fn empty_array_literal(ast: &Ast, mut e: ExprId) -> bool {
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    matches!(ast.get_expr(e), Expr::Array(els) if els.is_empty())
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

#[cfg(test)]
mod tests {
    fn check_src(src: &str) -> Result<(), String> {
        let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        crate::check::check_with_types(&ast).map(|_| ())
    }

    const XS: &str = "const xs: number[] = [1, 2];\nconst o: any = { sum: 0 };\n";

    fn refused(body: &str) {
        let err = check_src(&format!("{XS}{body}")).expect_err(body);
        assert!(err.contains("on `k`"), "{body}: {err}");
    }

    fn accepted(body: &str) {
        let r = check_src(&format!("{XS}{body}"));
        assert!(r.is_ok(), "{body}: {r:?}");
    }

    #[test]
    fn a_seeded_reducer_answers_its_accumulator_type() {
        let obj = "const a = xs.reduce((acc: { sum: number }, x: number): { sum: number } => \
                   ({ sum: acc.sum + x }), { sum: 0 });\n";
        accepted(&format!("{obj}const n: number = a.sum;\n"));
        refused(&format!("{obj}const k: string = a;\n"));
        refused(&format!("{obj}const k: string = a.sum;\n"));
        let s = "const s = xs.reduce((acc: string, x: number): string => acc + x, \"\");\n";
        accepted(&format!("{s}const k: string = s;\n"));
        refused(&format!("{s}const k: number = s;\n"));
    }

    #[test]
    fn an_any_return_or_seed_is_held_to_the_accumulator_annotation() {
        let ret = "const a = xs.reduce((acc: { sum: number }, x: number): any => \
                   ({ sum: acc.sum + x }), { sum: 0 });\n";
        refused(&format!("{ret}const k: string = a;\n"));
        let seed = "const a = xs.reduce((acc: { sum: number }, x: number): { sum: number } => \
                    ({ sum: acc.sum + x }), o);\n";
        refused(&format!("{seed}const k: string = a;\n"));
        let arr = "const a = xs.reduce((acc: number[], x: number): number[] => \
                   { acc.push(x); return acc; }, []);\n";
        refused(&format!("{arr}const k: string = a;\n"));
        accepted(&format!("{arr}const k: number[] = a;\n"));
    }

    #[test]
    fn a_scalar_accumulator_with_an_any_return_or_seed_stays_any() {
        let ret = "const a = xs.reduce((acc: number, x: number): any => acc + \"a\", 0);\n";
        accepted(&format!("{ret}const k: string = a;\n"));
        let seed = "const a = xs.reduce((acc: string, x: number): string => acc + x, o);\n";
        accepted(&format!("{seed}const k: number = a;\n"));
    }

    #[test]
    fn an_any_accumulator_or_no_seed_stays_any() {
        accepted("const a = xs.reduce((acc: any, x: number) => acc, 0);\nconst k: string = a;\n");
    }
}
