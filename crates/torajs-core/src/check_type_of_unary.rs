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
//!   (ToNumber(undefined) = NaN per §7.1.4); `BigInt` → the TypeError
//!   §7.1.4 mandates, **decided here and emitted** (band B — the
//!   program runs and throws, rather than being refused); `Any` →
//!   `Number` (ToNumber answers a Number or throws); a nullable
//!   number / boolean → `Any` (identity-Mul to ToNumber, mirrors
//!   Neg's any_arith).
//! - **`~`** (BitNot, V3-18 m1.f / spec §13.5.6) — ToInt32 (via
//!   ToNumber). `Number` → `Number`; `BigInt` → `BigInt` (V3-02:
//!   `~x` ≡ `-x - 1n`); `Boolean`/`Null`/`Undefined`/`String` →
//!   `Number` (Bool/Null clean i32; Undefined → NaN → ToInt32 = 0
//!   → `~0` = -1 per S136; String routes through
//!   `__torajs_str_to_number` per S137).

use crate::ast::{Ast, ExprId, UnaryOp};
use crate::check::{Checker, DiagPush, Type, js_truthy_acceptable, rides_any_lane};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    op: UnaryOp,
    expr: ExprId,
) -> Result<Type, String> {
    let t = checker.type_of(ast, expr)?;
    // Band B — §7.1.4 ToNumber refuses a BigInt outright, and unary
    // `+` is nothing but ToNumber. The operand type is known here, so
    // the outcome is too. The rejection this replaces said so itself:
    // "`+` on bigint is a TypeError per spec".
    if op == UnaryOp::Plus && t == Type::BigInt {
        // The message the any lane already raises for `+bigint`, so
        // the two lanes agree; it differs from bun's wording, which is
        // pre-existing and not what this change is about (§7.1.4
        // mandates the class, not the text).
        let msg = "Cannot mix BigInt and other types, use explicit conversions".to_string();
        checker.spec_throws.insert(eid, msg.clone());
        checker.errors.push_warn(msg);
        // `Number`, not `Any`: unary `+` produces a Number wherever it
        // produces anything, so the enclosing expression keeps its
        // typed lane (`function f(x: bigint): number { return +x }`
        // still returns into an f64 slot) and nothing widens. The
        // value is unreachable either way — see `ssa_lower_spec_throw`
        // on why its shape still matters.
        return Ok(Type::Number);
    }
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
        // §13.5.4: `+x` is ToNumber(x), whose only answers are a
        // Number or a throw, so an `any` operand still yields a
        // Number. A `T | null` keeps the any lane's shape.
        UnaryOp::Plus if t == Type::Any => Ok(Type::Number),
        UnaryOp::Plus if rides_any_lane(&t) => Ok(Type::Any),
        UnaryOp::Plus => check_plus(t),
        UnaryOp::BitNot => check_bit_not(t),
    }
}

fn check_neg(t: Type) -> Result<Type, String> {
    if t == Type::Number {
        Ok(Type::Number)
    } else if t == Type::BigInt {
        Ok(Type::BigInt)
    } else if is_object_shaped(&t) {
        // §13.5.5 step 2 is ToNumeric, and an object's answer may be
        // a BigInt (`-Object(1n)` is `-1n`, `-{ valueOf() { return 1n
        // } }` too — rotation 629, test262 unary-minus/bigint-non-
        // primitive), so the promise is the any lane's, whose negate
        // kernel steps in whichever domain ToNumeric lands in.
        Ok(Type::Any)
    } else if is_number_coercible(&t) {
        Ok(Type::Number)
    } else if rides_any_lane(&t) {
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
            | Type::Map(..)
            | Type::Set
            | Type::MapIter(..)
            | Type::ArrIter
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::RegExp
            | Type::Date
            | Type::Promise(_)
    )
}

/// The BigInt arm that used to live here is gone: `check` decides it
/// above and emits the throw (band B), so this is only ever reached
/// for the shapes ToNumber can answer.
fn check_plus(t: Type) -> Result<Type, String> {
    if t == Type::Number || is_number_coercible(&t) {
        Ok(Type::Number)
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
    } else if rides_any_lane(&t) {
        // RFC 20260716 刀 7 — Any operand routes through
        // `__torajs_anyv_bitnot_pair` (ToNumber → ToInt32 → xor -1),
        // matching arith / compare Any-fringe pattern.
        Ok(Type::Any)
    } else {
        Err(format!("`~` requires number or bigint operand, got {t:?}"))
    }
}
