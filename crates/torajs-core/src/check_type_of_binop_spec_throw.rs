//! Which operand pairs ECMA-262 answers with a **throw** rather than a
//! value — band B's entry test, split out of
//! [`crate::check_type_of_binop`] because it is a different question
//! from "what type does this operator produce".
//!
//! The parent asks what the result is; this file asks whether there is
//! a result at all. Keeping them apart matters because the answers
//! come from different places: the parent's lanes are tr's own type
//! rules, and every line here is a clause of ECMA-262 checked against
//! bun.

use crate::ast::BinOp;
use crate::check::Type;

/// The operand pairs ECMA-262 answers with a throw rather than a value.
///
/// **Narrow on purpose.** Everything else this file rejects is a lane
/// the checker has not been taught — a gap, not a throw the spec asks
/// for — and band B is only the second kind. Widening this by guessing
/// would manufacture TypeErrors for programs that should run, which is
/// the failure mode `ch1 §5.1` records: the error class cannot be
/// recovered from a rejection message, it has to be decided where the
/// rejection is written.
///
/// Every message here is the one the runtime kernels already raise for
/// the same operands (`torajs-anyvalue/src/arith_bigint.rs`,
/// `arith_operand.rs`, `coerce.rs`) — and those agree with bun
/// verbatim. Deciding at compile time changes when the work happens,
/// not what the program prints.
///
/// The shape of the rule, measured against bun rather than reasoned
/// from the prose:
///
/// | pair | `+` | other numeric ops |
/// |---|---|---|
/// | BigInt × Number / Boolean / Null / Undefined | throws | throws |
/// | BigInt × String | **concatenates** | throws |
/// | Symbol × String | throws (…to a string) | throws (…to a number) |
/// | Symbol × anything else | throws (…to a number) | throws |
/// | BigInt × BigInt | fine | fine (except `>>>`) |
pub(crate) fn spec_mandated_throw(op: BinOp, l: &Type, r: &Type) -> Option<String> {
    const SYM_NUM: &str = "Cannot convert a symbol to a number";
    const SYM_STR: &str = "Cannot convert a symbol to a string";
    const MIXED: &str = "Cannot mix BigInt and other types, use explicit conversions";

    let is_str = |t: &Type| matches!(t, Type::String);
    let is_sym = |t: &Type| matches!(t, Type::Symbol);

    // §7.2.13 IsLessThan hands a Symbol back from ToPrimitive and then
    // ToNumeric refuses it. This is the ONLY way a comparison throws:
    // the numeric types themselves compare across (step 5 puts a
    // BigInt against a Number by mathematical value), which is why
    // comparison is otherwise absent from this file.
    if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) && (is_sym(l) || is_sym(r)) {
        return Some(SYM_NUM.into());
    }

    if !coerces_numerically(op) {
        return None;
    }

    // §7.1.1 ToPrimitive hands a Symbol straight back, and both
    // ToNumeric and ToString then refuse it. `+` reaches ToString only
    // when the OTHER side is a String; everything else asks for a
    // number.
    if is_sym(l) || is_sym(r) {
        let to_string = op == BinOp::Add && (is_str(l) || is_str(r));
        return Some(if to_string { SYM_STR } else { SYM_NUM }.into());
    }

    // §13.15.3 step 3: `+` concatenates as soon as either ToPrimitive
    // result is a String, so a String side never reaches ToNumeric and
    // never disagrees with a BigInt. Every other operator skips
    // straight to ToNumeric, where it does.
    if op == BinOp::Add && (is_str(l) || is_str(r)) {
        return None;
    }

    // §13.12 — `>>>` has no BigInt form at all, so it throws even when
    // both sides are BigInt.
    if op == BinOp::UShr && (*l == Type::BigInt || *r == Type::BigInt) {
        return Some(MIXED.into());
    }

    // "If Type(lnum) is not Type(rnum), throw a TypeError" — the two
    // numeric types after ToNumeric, and neither coerces to the other.
    match (to_numeric_kind(l), to_numeric_kind(r)) {
        (Some(a), Some(b)) if a != b => Some(MIXED.into()),
        _ => None,
    }
}

/// The operators whose evaluation reaches ToNumeric / ToPrimitive on
/// both operands. Comparison and equality are deliberately absent:
/// §7.2.13 IsLessThan and §7.2.15 IsStrictlyEqual are defined ACROSS
/// the numeric types and answer rather than throw.
fn coerces_numerically(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr
    )
}

/// Which of the two numeric types §7.1.4 / §7.1.13 lands this type on,
/// or `None` when the answer depends on a value the checker cannot see
/// (an object's `valueOf`, an `any`).
fn to_numeric_kind(t: &Type) -> Option<bool> {
    match t {
        // `true` = BigInt side.
        Type::BigInt => Some(true),
        Type::Number | Type::Boolean | Type::Null | Type::Undefined | Type::String => Some(false),
        _ => None,
    }
}
