//! `Expr::Unary` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Unary` arm
//! as chunk-89 of the type_of_inner decomp.
//!
//! Per-op result type rules (all routed through ToNumber/ToBoolean
//! coercions in ssa_lower's matching arms):
//!
//! - **`!`** (Not, V3-18 m1.h.2) — ToBoolean per §13.5.7; result
//!   `Boolean`. Accept any truthy-coercible type
//!   ([`js_truthy_acceptable`]).
//! - **`-`** (Neg, V3-18 m1.f / spec §13.5.5) — ToNumber per §7.1.4.
//!   `Number` → `Number`; `BigInt` → `BigInt` (T-25 bigint_neg);
//!   `Boolean`/`Null`/`String`/`Undefined` → `Number` (Bool/Null
//!   via m1.b coerce; String via `__torajs_str_to_number`; Undefined
//!   → NaN per S136); `Any` (P0.9) → `Any` (any_arith Sub with
//!   LHS=0).
//! - **`+`** (Plus, V3-18 m1.h.4 / spec §13.5.4) — ToNumber.
//!   `Number`/`Boolean`/`Null`/`String`/`Undefined` → `Number`
//!   (ToNumber(undefined) = NaN per §7.1.4); `BigInt` → TypeError
//!   (spec — runtime support unnecessary, caught here); `Any` (P0.9)
//!   → `Any` (identity-Mul to ToNumber, mirrors Neg's any_arith).
//! - **`~`** (BitNot, V3-18 m1.f / spec §13.5.6) — ToInt32 (via
//!   ToNumber). `Number` → `Number`; `BigInt` → `BigInt` (V3-02:
//!   `~x` ≡ `-x - 1n`); `Boolean`/`Null`/`Undefined`/`String` →
//!   `Number` (Bool/Null clean i32; Undefined → NaN → ToInt32 = 0
//!   → `~0` = -1 per S136; String routes through
//!   `__torajs_str_to_number` per S137).

use crate::ast::{Ast, ExprId, UnaryOp};
use crate::check::{Checker, Type, js_truthy_acceptable};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    op: UnaryOp,
    expr: ExprId,
) -> Result<Type, String> {
    let t = checker.type_of(ast, expr)?;
    match op {
        UnaryOp::Not => {
            if js_truthy_acceptable(&t) {
                Ok(Type::Boolean)
            } else {
                Err(format!(
                    "`!` requires boolean (or coercible) operand, got {t:?}"
                ))
            }
        }
        UnaryOp::Neg => check_neg(t),
        UnaryOp::Plus if matches!(t, Type::Any) => Ok(Type::Any),
        UnaryOp::Plus => check_plus(t),
        UnaryOp::BitNot => check_bit_not(t),
    }
}

fn check_neg(t: Type) -> Result<Type, String> {
    if t == Type::Number {
        Ok(Type::Number)
    } else if t == Type::BigInt {
        Ok(Type::BigInt)
    } else if is_number_coercible(&t) {
        Ok(Type::Number)
    } else if matches!(t, Type::Any) {
        Ok(Type::Any)
    } else {
        Err(format!("`-` requires number or bigint operand, got {t:?}"))
    }
}

/// §7.1.4 ToNumber has an answer for every one of these — a
/// primitive's own rule, or OrdinaryToPrimitive reaching the object's
/// `valueOf` / `toString` (NaN when neither says a number). So `+x`
/// and `-x` are legal source over all of them and the coercion is the
/// runtime's to do.
///
/// Only Symbol and BigInt are left out, where ToNumber THROWS
/// (§7.1.4 steps 2 and 5) — a compile error there matches TypeScript,
/// which rejects the same two. Every other object shape used to be
/// rejected as well, so `+xs` on a `number[]` did not build while
/// `+({} as any)` did; §7.1.4 defines both.
fn is_number_coercible(t: &Type) -> bool {
    matches!(
        t,
        Type::Boolean | Type::Null | Type::String | Type::Undefined
    ) || is_object_shaped(t)
}

/// The object half of [`is_number_coercible`] — every shape whose
/// ToNumber is OrdinaryToPrimitive over the receiver rather than a
/// primitive's own rule.
///
/// The binary lanes ask the same question (`objectish` in
/// `check_type_of_binop`), so it is one list: a shape admitted here
/// and forgotten there is how `+xs` and `xs * 1` came to disagree.
pub(crate) fn is_object_shaped(t: &Type) -> bool {
    matches!(
        t,
        Type::Struct(_)
            | Type::ClassRef(_)
            | Type::Array(_)
            | Type::Function(_, _)
            | Type::Object(_)
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::RegExp
            | Type::Date
            | Type::Promise(_)
    )
}

fn check_plus(t: Type) -> Result<Type, String> {
    if t == Type::Number || is_number_coercible(&t) {
        Ok(Type::Number)
    } else if t == Type::BigInt {
        Err("`+` on bigint is a TypeError per spec; use Number(x) for explicit coercion".into())
    } else {
        Err(format!(
            "`+` requires number or coercible operand, got {t:?}"
        ))
    }
}

fn check_bit_not(t: Type) -> Result<Type, String> {
    if t == Type::Number {
        Ok(Type::Number)
    } else if t == Type::BigInt {
        Ok(Type::BigInt)
    } else if matches!(
        t,
        Type::Boolean | Type::Null | Type::Undefined | Type::String
    ) {
        Ok(Type::Number)
    } else if matches!(t, Type::Any) {
        // RFC 20260716 刀 7 — Any operand routes through
        // `__torajs_anyv_bitnot_pair` (ToNumber → ToInt32 → xor -1),
        // matching arith / compare Any-fringe pattern.
        Ok(Type::Any)
    } else {
        Err(format!("`~` requires number or bigint operand, got {t:?}"))
    }
}
