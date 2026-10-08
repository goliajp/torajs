//! The ordering operators of the BinOp checker (`<` `>` `<=` `>=`,
//! §7.2.13 IsLessThan), carved out of `check_type_of_binop.rs`.

use crate::check::{Type, js_arith_coerces_to_number, rides_any_lane};

use super::{objectish_numeric_pair, string_numeric_pair};

pub(super) fn check_compare(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Boolean);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::Boolean);
    }
    // §7.2.13 IsLessThan is defined ACROSS the numeric types: steps
    // 3.a-3.b take a BigInt against a String (via StringToBigInt) and
    // steps 4-5 take a BigInt against a Number by mathematical value.
    // Only the ARITHMETIC operators refuse a mixed pair (§6.1.6.1.x),
    // and reading that refusal onto comparison made `1n < 2` a
    // compile error while `(1n as any) < 2` answered true off the
    // same kernel.
    // …and Boolean / Null / Undefined reach step 4 the same way a
    // Number does (ToNumeric answers 1/0, 0 and NaN), so they compare
    // against a BigInt too. Listing only Number and String here made
    // `1n < true` a compile error while `(1n as any) < true` answered
    // false off the same kernel — the same half-measure the rest of
    // this family had.
    let cross = |t: &Type| {
        matches!(
            t,
            Type::Number | Type::String | Type::Boolean | Type::Null | Type::Undefined
        )
    };
    if matches!(l, Type::BigInt) && cross(&r) || matches!(r, Type::BigInt) && cross(&l) {
        return Ok(Type::Boolean);
    }
    if rides_any_lane(&l) || rides_any_lane(&r) {
        return Ok(Type::Boolean);
    }
    if l == Type::String && r == Type::String {
        return Ok(Type::Boolean);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Boolean);
    }
    // §13.10 ToPrimitive with the NUMBER hint on both sides, then the
    // numeric comparison — `any_compare` carries it, same as for an
    // `any` operand.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Boolean);
    }
    // §7.2.13 — the String/String arm above already took the
    // lexicographic branch, so past it a String side is ToNumeric'd
    // like every other operand; `any_compare` carries it.
    if string_numeric_pair(&l, &r) {
        return Ok(Type::Boolean);
    }
    Err(format!(
        "ordering comparison requires number or bigint operands, got {l:?} and {r:?}"
    ))
}
