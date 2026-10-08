//! The equality and logical operators of the BinOp checker
//! (`==` / `===` and their negations, `&&` / `||`), carved out of
//! `check_type_of_binop.rs` when the arithmetic side's BigInt
//! verdict (rotation 629) took the file past the 500-line cap. The
//! parent keeps the numeric family (add / arith / pow / bitwise;
//! ordering is in `compare`) and dispatches here for these six.

use crate::ast::{Ast, BinOp};
use crate::check::{Type, js_loose_eq_supported};

pub(super) fn check_equality(op: BinOp, l: Type, r: Type) -> Result<Type, String> {
    if l == r
        && matches!(
            l,
            Type::Number | Type::String | Type::Boolean | Type::BigInt
        )
    {
        return Ok(Type::Boolean);
    }
    // T-13.a — Symbol === Symbol is pointer identity.
    if l == r && matches!(l, Type::Symbol) {
        return Ok(Type::Boolean);
    }
    // Nullable/Null cross-pair.
    let l_is_null = matches!(l, Type::Null | Type::Undefined);
    let r_is_null = matches!(r, Type::Null | Type::Undefined);
    let l_is_nullable = matches!(l, Type::Nullable(_));
    let r_is_nullable = matches!(r, Type::Nullable(_));
    if (l_is_null || l_is_nullable) && (r_is_null || r_is_nullable) {
        return Ok(Type::Boolean);
    }
    // V3-18 m3 — LooseEq cross-type coercion family.
    if matches!(op, BinOp::LooseEq | BinOp::LooseNeq) && js_loose_eq_supported(&l, &r) {
        return Ok(Type::Boolean);
    }
    // V3-18 m3.b — Eq/Neq cross-type per §7.2.15: different types →
    // false (no throw). Accept any pair; ssa_lower static-folds.
    if matches!(op, BinOp::Eq | BinOp::Neq) {
        return Ok(Type::Boolean);
    }
    Err(format!(
        "strict equality requires same primitive type, got {l:?} and {r:?}"
    ))
}

pub(super) fn check_logical(ast: &Ast, op: BinOp, l: Type, r: Type) -> Result<Type, String> {
    if l == r {
        return Ok(l);
    }
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    if matches!(l, Type::Null | Type::Undefined) {
        // S138 — statically falsy lhs.
        return Ok(match op {
            BinOp::LOr => r,
            BinOp::LAnd => l,
            _ => unreachable!(),
        });
    }
    if matches!(&l, Type::Nullable(linner) if **linner == r) {
        // `Nullable<T> || T` per §13.13 (and `&&` mirror).
        return Ok(match op {
            BinOp::LOr => r,
            BinOp::LAnd => l,
            _ => unreachable!(),
        });
    }
    if matches!(&r, Type::Nullable(rinner) if **rinner == l) {
        // `T || Nullable<T>` per §13.13 (and `&&` mirror).
        return Ok(match op {
            BinOp::LOr | BinOp::LAnd => r,
            _ => unreachable!(),
        });
    }
    // Rotation 507 — two DIFFERENT class instances meet at their
    // nearest common ancestor, the ternary / `??` join (`m || new
    // Leaf(2)` with `m: Mid | null` used to reject where bun runs
    // it). Either side unwraps its `Nullable` first: `||` answers the
    // rhs exactly when the lhs is falsy, so the join is over what
    // each side can BE.
    let core = |t: &Type| match t {
        Type::Nullable(i) => (**i).clone(),
        other => other.clone(),
    };
    if let Some(joined) = crate::check_type_compare::class_join(ast, &core(&l), &core(&r)) {
        return Ok(joined);
    }
    // §13.13 — `a && b` IS one of its two operands: `a` when `a` is
    // falsy, `b` otherwise. Every pair of types is therefore legal,
    // and what the rejected ones needed was a name for the join. With
    // no general union type to spell `A | B`, that name is Any --
    // which is what `Nullable<T> || T` above already settles for in
    // its own small way. Refusing instead meant `flag && count`,
    // `s || 0`, `xs.length && xs[0]` -- shapes JS is written in --
    // did not compile, with a message about "matching operand types"
    // that no rule of the language asks for.
    //
    // Only the previously-rejected pairs land here: every arm above
    // still answers its exact type, so nothing that compiled before
    // loses precision. `ssa_lower_logical` widens the join slot to
    // Any on the same condition and boxes both arms into it.
    Ok(Type::Any)
}
