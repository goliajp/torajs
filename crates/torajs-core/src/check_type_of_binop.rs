//! `Expr::BinOp { op, left, right }` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::BinOp` arm as
//! chunk-96 of the type_of_inner decomp.
//!
//! 8 op-family helpers — each a pure type-math function on the two
//! operand types (and `op` itself for the families with multiple
//! variants):
//!
//! - **Add** — spec §13.15.3 ApplyStringOrNumericBinaryOperator.
//!   Number+Number → Number; BigInt+BigInt → BigInt; String+String
//!   / String+Number / String+BigInt / String+Bool/Null/Undef /
//!   String+Array/Struct / String+Any → String (V3-18 m1.d / S138 /
//!   m3.c); Any+other → Any (P0.6); Bool/Null pairs → Number via
//!   `js_add_coerces_to_number` (m1.a).
//! - **Sub/Mul/Div/Mod** — spec §13.6-§13.9. Number/BigInt identity;
//!   Any → Any (P0.7); Bool/Null → Number via
//!   `js_arith_coerces_to_number` (m1.b).
//! - **Pow** (V3-01) — Number/BigInt identity only; mixed → spec
//!   TypeError.
//! - **BitAnd/BitOr/BitXor/Shl/Shr** — spec §13.12 ToInt32 both
//!   sides. Number/BigInt identity; Bool/Null via
//!   `js_arith_coerces_to_number` (m1.e).
//! - **UShr** — same as Shr but BigInt → spec TypeError per V3-02.
//! - **Lt/Gt/Le/Ge** — spec §7.2.14 IsLessThan. Number/BigInt →
//!   Boolean; Any → Boolean (P0.8); String/String → Boolean
//!   (m1.h.17); Bool/Null → Boolean via `js_arith_coerces_to_number`
//!   (m1.c).
//! - **Eq/Neq/LooseEq/LooseNeq** — same primitive type → Boolean;
//!   Symbol/Symbol pointer-identity → Boolean (T-13.a);
//!   Nullable+Null cross → Boolean; LooseEq cross-type via
//!   `js_loose_eq_supported` (m3); Eq/Neq cross-type → Boolean per
//!   spec §7.2.15 (m3.b — false / true static fold at ssa_lower).
//! - **LAnd/LOr** — spec §13.13. T && T → T; Any → Any;
//!   Null/Undef lhs → rhs/lhs by op; Nullable<T> lhs + rhs T → T
//!   (S138 + mirror); T + Nullable<T> rhs → Nullable<T>.

use crate::ast::{Ast, BinOp, ExprId};
use crate::check::{
    Checker, Type, js_add_coerces_to_number, js_arith_coerces_to_number, js_loose_eq_supported,
};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    op: BinOp,
    left: ExprId,
    right: ExprId,
) -> Result<Type, String> {
    let l = checker.type_of(ast, left)?;
    let r = checker.type_of(ast, right)?;
    match op {
        BinOp::Add => check_add(l, r),
        BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => check_arith(l, r),
        BinOp::Pow => check_pow(l, r),
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
            check_bitwise(l, r)
        }
        BinOp::UShr => check_ushr(l, r),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => check_compare(l, r),
        BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq => check_equality(op, l, r),
        BinOp::LAnd | BinOp::LOr => check_logical(op, l, r),
    }
}

fn check_add(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::BigInt);
    }
    // String concat family: Str+Str / Str+Number / Str+BigInt /
    // Str+Bool/Null/Undef / Str+Array/Struct / Str+Any.
    if (l == Type::String && r == Type::String)
        || (l == Type::String && r == Type::Number)
        || (l == Type::Number && r == Type::String)
        || (l == Type::String && r == Type::BigInt)
        || (l == Type::BigInt && r == Type::String)
        || (l == Type::String && matches!(r, Type::Boolean | Type::Null | Type::Undefined))
        || (matches!(l, Type::Boolean | Type::Null | Type::Undefined) && r == Type::String)
        // A ClassRef sits beside Struct: §13.15.3 ToPrimitive reaches
        // the instance's `toString` the same way, and the concat lane
        // hands both to the same runtime kernel.
        || (l == Type::String && matches!(r, Type::Array(_) | Type::Struct(_) | Type::ClassRef(_)))
        || (matches!(l, Type::Array(_) | Type::Struct(_) | Type::ClassRef(_)) && r == Type::String)
        || (l == Type::String && matches!(r, Type::Any))
        || (matches!(l, Type::Any) && r == Type::String)
        // RFC 20260719-fn-tostring-source B5 — Str + fn concat:
        // ToPrimitive(fn) → toString() → the erased source text.
        || (l == Type::String && matches!(r, Type::Function(..)))
        || (matches!(l, Type::Function(..)) && r == Type::String)
        // Rotation 437 — Str + RegExp: §13.15.3 ToPrimitive reaches
        // §22.2.6.14 `toString` → "/source/flags". The regex-literal
        // binding already concatenated through this family; the
        // `new RegExp(...)` spelling types Type::RegExp and was the
        // one shape left refusing (the S15.10.1_A1 error-message
        // interpolations).
        || (l == Type::String && r == Type::RegExp)
        || (l == Type::RegExp && r == Type::String)
        // Cluster #6 (rotation 442) — Str + Nullable(Array): the
        // un-narrowed `match`/`exec` result rides the concat lane
        // directly (`"got: " + s.match(re)`). §13.15.3 with a String
        // side always concatenates, and ToString covers both arms:
        // null → "null", an array → its join — so the answer is
        // String either way. The lowering guards the in-band 0
        // sentinel before the array coerce.
        || (l == Type::String && matches!(&r, Type::Nullable(inner) if matches!(&**inner, Type::Array(_))))
        || (matches!(&l, Type::Nullable(inner) if matches!(&**inner, Type::Array(_))) && r == Type::String)
    {
        return Ok(Type::String);
    }
    // P0.6 — Any operand on either side: ssa_lower routes through
    // __torajs_any_add (ToPrimitive Default → concat or numeric add).
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    if js_add_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.15.3 ToPrimitive with the DEFAULT hint, which for an object
    // reaches `valueOf` first and `toString` after — so `{valueOf(){return 42}} + 1`
    // is 43 while a hook-free object concatenates. Which one it is
    // cannot be known here, so this answers Any and lowering hands both
    // sides to `any_add`, the kernel that already carries the whole
    // walk (it is what an `any`-typed operand has always used).
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    Err(format!(
        "`+` requires matching number/string/bigint operands or string+number, got {l:?} and {r:?}"
    ))
}

/// One side is an object, and the other is one the numeric lanes can
/// describe — the shape every binary operator sends through
/// ToPrimitive before doing anything else (§13.15.3 for `+`, §13.10
/// for ordering, §13.12 for bitwise, §13.7 for the rest).
///
/// A String on the other side is deliberately NOT admitted here. For
/// `+` it already has a concat arm that this must not shadow; for the
/// ordering operators it would need §13.10's string-comparison branch
/// rather than a numeric one, which is a different question and stays
/// a loud reject until someone answers it.
fn objectish_numeric_pair(l: &Type, r: &Type) -> bool {
    let objectish = |t: &Type| matches!(t, Type::Struct(_) | Type::ClassRef(_));
    (objectish(l) || objectish(r))
        && (objectish(l) || *l == Type::Number)
        && (objectish(r) || *r == Type::Number)
}

fn check_arith(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::BigInt);
    }
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.7-§13.10 call ToNumeric on each operand unconditionally, so
    // an object side runs its `valueOf` and answers NaN when it has
    // none. This arm can promise Number because ToNumeric has no other
    // answer; the operators whose ToPrimitive can hand back a string
    // (`+`) or needs a string branch (ordering) route to the any-lane
    // kernels instead — see `objectish_numeric_pair`.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Number);
    }
    Err(format!(
        "arithmetic requires number or bigint operands, got {l:?} and {r:?}"
    ))
}

fn check_pow(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::BigInt);
    }
    // S2.43 — Any operand on either side: ssa_lower routes through
    // `__torajs_anyv_arith_pair` op 4. The rotation-240 BigInt-side
    // reject is retired: the kernel now carries the ToNumeric
    // dispatch (a BigInt pair rides `__torajs_bigint_pow`, a mixed
    // pair records the catchable mixed-types TypeError), so
    // `2n ** (p: any)` resolves at runtime per §13.6 instead of a
    // compile-time refusal.
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    // §13.6 is ToNumeric both sides, which reaches `valueOf`; the arith
    // kernel's op 4 takes it exactly as it does for an `any` operand.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    Err(format!(
        "`**` requires matching number or bigint operands, got {l:?} and {r:?}"
    ))
}

fn check_bitwise(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::BigInt);
    }
    // RFC 20260716 刀 7 — Any operand on either side: ssa_lower
    // routes through `__torajs_anyv_bitwise_pair` (ES §13.12
    // ToInt32/ToUint32 both sides, then 32-bit op). Mirrors the
    // arith / compare Any-fringe pattern.
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.12 ToInt32/ToUint32 both sides, which reaches `valueOf`;
    // `any_bitwise` takes it as it does for an `any` operand.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    Err(format!(
        "bitwise op requires matching number or bigint operands, got {l:?} and {r:?}"
    ))
}

fn check_ushr(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt || r == Type::BigInt {
        return Err("`>>>` is not defined on BigInt operands per spec".into());
    }
    // RFC 20260716 刀 7 — Any operand routes to any_bitwise per
    // §13.12 (op=5 UShr — LHS ToUint32, result promoted to F64 if
    // ≥ 2^31).
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        return Ok(Type::Any);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.12 op 5 — same ToUint32 walk, same kernel.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    Err(format!(
        "bitwise op requires number operands, got {l:?} and {r:?}"
    ))
}

fn check_compare(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Boolean);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::Boolean);
    }
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
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
    Err(format!(
        "ordering comparison requires number or bigint operands, got {l:?} and {r:?}"
    ))
}

fn check_equality(op: BinOp, l: Type, r: Type) -> Result<Type, String> {
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

fn check_logical(op: BinOp, l: Type, r: Type) -> Result<Type, String> {
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
    Err(format!(
        "`&&` / `||` require matching operand types, got {l:?} and {r:?}"
    ))
}
