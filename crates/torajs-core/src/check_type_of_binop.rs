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
    Checker, DiagPush, Type, js_add_coerces_to_number, js_arith_coerces_to_number, rides_any_lane,
};
use crate::check_js_semantics::nullable_ptr_shaped;

mod eq_logical;
use eq_logical::{check_equality, check_logical};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    op: BinOp,
    operands: (ExprId, ExprId),
) -> Result<Type, String> {
    let (left, right) = operands;
    // Both operands are TYPED before either failure propagates.
    // `type_of` records into `expr_types` only on Ok, and the `?` on
    // the left used to skip the right's visit entirely — so in a
    // monomorph specialization body (whose diagnostics are discarded
    // by design, `check_monomorph_uninferred`) a left operand the
    // checker rejects left the right operand with NO recorded type.
    // `lower_binop_with_ids` reads exactly that entry to tell
    // `=== undefined` (ANY_UNDEF tag 5) from `=== null` (tag 0), so
    // `o.missing === undefined` inside `function f(o) { ... }` packed
    // the null tag and answered FALSE — the guard read backwards, and
    // `!== undefined` always true.
    let l = checker.type_of(ast, left);
    let r = checker.type_of(ast, right);
    let (l, r) = (l?, r?);
    // Band B before the per-op lanes: when the spec answers this pair
    // with a throw and both types are known, the answer is decided
    // HERE. The program still compiles and still throws — see
    // `Ast::spec_throws` and `docs/language-spec/ch1-type-runtime-boundary.md`.
    if let Some(msg) = crate::check_type_of_binop_spec_throw::spec_mandated_throw(op, &l, &r) {
        checker.spec_throws.insert(eid, msg.clone());
        checker.errors.push_warn(msg);
        return Ok(Type::Any);
    }
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
        BinOp::LAnd | BinOp::LOr => check_logical(ast, op, l, r),
    }
}

/// §13.15.3 with a String side is a concat, and ToString answers for
/// every one of these: the any-lane kernel reads the heap header tag
/// and gives "[object Map]" / "[object Promise]" / the Date string /
/// the two iterator names. `("x" as any) + new Map()` has printed
/// them all along; the static lane listed only RegExp, so the same
/// program with the `any` erased was a type error.
///
/// Symbol is deliberately absent — §13.15.3 ToString of a Symbol
/// throws, and tsc refuses `string + symbol` as well.
fn heap_cell_tostring(t: &Type) -> bool {
    matches!(
        t,
        Type::RegExp
            | Type::Map(..)
            | Type::Set
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::MapIter(..)
            | Type::ArrIter
            | Type::Date
            | Type::Promise(_)
    )
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
        || (l == Type::String && heap_cell_tostring(&r))
        || (heap_cell_tostring(&l) && r == Type::String)
        // Cluster #6 (rotation 442) — Str + a pointer-shaped
        // nullable. §13.15.3 with a String side always concatenates,
        // and ToString answers for BOTH faces of the union: null →
        // "null", the value → whatever its own ToString says. So the
        // result is String either way, and the reason has nothing to
        // do with which T it is — the arm was written for the
        // un-narrowed `match`/`exec` result (`"got: " + s.match(re)`)
        // and stayed pinned to Array for three hundred rotations,
        // refusing `"" + q` on a struct, a string and a closure alike.
        // The two scalars are not here: their nullable rides `any`
        // and the arm below answers them.
        || (l == Type::String && nullable_ptr_shaped(&r))
        || (nullable_ptr_shaped(&l) && r == Type::String)
    {
        return Ok(Type::String);
    }
    // §13.15.3 with a nullable STRING on one side and anything but a
    // String on the other: the union's two faces disagree about the
    // operator — `"a" + undefined` concatenates while
    // `undefined + undefined` is NaN — so the answer is the one the
    // any lane's kernel decides at run time. The String pairs answer
    // above (both faces concatenate).
    if (nullable_str(&l) && r != Type::String) || (nullable_str(&r) && l != Type::String) {
        return Ok(Type::Any);
    }
    // P0.6 — Any operand on either side: ssa_lower routes through
    // __torajs_any_add (ToPrimitive Default → concat or numeric add).
    if rides_any_lane(&l) || rides_any_lane(&r) {
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

/// `string | undefined` / `string | null` — the union whose two faces
/// answer different operators.
fn nullable_str(t: &Type) -> bool {
    matches!(t, Type::Nullable(inner) if **inner == Type::String)
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
    let objectish = crate::check_type_of_unary::is_object_shaped;
    // Rotation 628 — a BigInt on the other side is the same ToNumeric
    // question: `Object(5n) & 3n` / `{ valueOf() { return 5n } } * 2n`
    // reach `valueOf` and step in the BigInt domain (§13.12 step 3 /
    // §6.1.6.2); the any kernels already carry that pair.
    let numeric = |t: &Type| *t == Type::Number || *t == Type::BigInt;
    (objectish(l) || objectish(r)) && (objectish(l) || numeric(l)) && (objectish(r) || numeric(r))
}

/// One side is a String and both sides coerce through ToNumeric —
/// which on a String is ToNumber (never BigInt), so `"3" & 1` is `1`,
/// `1 - "2"` is `-1`, `"abc" * 2` is `NaN` and `"2" < 3` is `true`,
/// none of them a compile-time refusal. BigInt on the other side
/// stays out: §13.12 step 3 / §6.1.6.1.x make a mixed pair a runtime
/// TypeError the static lane doesn't carry.
///
/// This arm reached the bitwise six first and was written "bitwise
/// ONLY", but the reason it was held back from the ARITHMETIC family
/// belonged to `+` alone: `+` has a concat path (§13.15.3 picks it
/// when either side is a String) and ordering has §13.10's
/// string-comparison branch. `- * / %` and `**` have neither —
/// §13.7-§13.10 and §13.6 step 1 call ToNumeric unconditionally.
/// They inherited the exclusion through `js_arith_coerces_to_number`,
/// which is `js_add_coerces_to_number` verbatim, so `"2" * 3` was a
/// compile error while `("2" as any) * 3` answered 6 off the very
/// kernel this arm routes to. Ordering keeps its own String/String
/// arm above; past it, §7.2.13 is numeric on both sides.
fn string_numeric_pair(l: &Type, r: &Type) -> bool {
    let ok = |t: &Type| {
        matches!(
            t,
            Type::String | Type::Number | Type::Boolean | Type::Null | Type::Undefined
        )
    };
    (*l == Type::String || *r == Type::String) && ok(l) && ok(r)
}

fn check_arith(l: Type, r: Type) -> Result<Type, String> {
    if l == Type::Number && r == Type::Number {
        return Ok(Type::Number);
    }
    if l == Type::BigInt && r == Type::BigInt {
        return Ok(Type::BigInt);
    }
    if rides_any_lane(&l) || rides_any_lane(&r) {
        return Ok(Type::Any);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.7-§13.10 call ToNumeric on each operand unconditionally, so
    // an object side runs its `valueOf` and answers NaN when it has
    // none. Beside a Number the promise is Number (a BigInt answer
    // from the object is the mixed-types TypeError, never a value);
    // beside a BigInt or another object the value the runtime answers
    // may be a BigInt (`{ valueOf() { return 2n } } / 2n` is `1n` —
    // rotation 629, test262 `expressions/division/bigint-toprimitive`
    // and its family, which reach `assert.sameValue(obj / 2n, 1n)`
    // and died on the generic's Number-vs-BigInt conflict), so the
    // static answer is the any lane's, which is the kernel these
    // pairs already ride. The operators whose ToPrimitive can hand
    // back a string (`+`) or need a string branch (ordering) route
    // there too — see `objectish_numeric_pair`.
    if objectish_numeric_pair(&l, &r) {
        return Ok(if l == Type::Number || r == Type::Number {
            Type::Number
        } else {
            Type::Any
        });
    }
    // §13.7-§13.10 step 1 is ToNumeric on both, and a String has an
    // answer there — the any-lane arith kernel already gives it.
    if string_numeric_pair(&l, &r) {
        return Ok(Type::Any);
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
    if rides_any_lane(&l) || rides_any_lane(&r) {
        return Ok(Type::Any);
    }
    // §13.6 step 1 is ToNumeric on both operands, the same call
    // §13.7-§13.10 make — so `**` takes the same coercibles its four
    // arithmetic siblings take. It was the one operator in the family
    // that never got this arm, which is why `null ** 2` and even
    // `true ** 2` were compile errors while `null * 2` ran.
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.6 is ToNumeric both sides, which reaches `valueOf`; the arith
    // kernel's op 4 takes it exactly as it does for an `any` operand.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    // §13.6 step 1 is the same ToNumeric call; same kernel.
    if string_numeric_pair(&l, &r) {
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
    if rides_any_lane(&l) || rides_any_lane(&r) {
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
    // §13.12 — a String side is ToNumber'd; same any_bitwise route.
    if string_numeric_pair(&l, &r) {
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
    // RFC 20260716 刀 7 — Any operand routes to any_bitwise per
    // §13.12 (op=5 UShr — LHS ToUint32, result promoted to F64 if
    // ≥ 2^31).
    if rides_any_lane(&l) || rides_any_lane(&r) {
        return Ok(Type::Any);
    }
    if js_arith_coerces_to_number(&l, &r) {
        return Ok(Type::Number);
    }
    // §13.12 op 5 — same ToUint32 walk, same kernel.
    if objectish_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    // §13.12 — a String side is ToNumber'd; same any_bitwise route.
    if string_numeric_pair(&l, &r) {
        return Ok(Type::Any);
    }
    // §13.12 — `>>>` has no BigInt form. A pair of PRIMITIVES never
    // arrives here (band B decided it at the entry and recorded the
    // throw), so what is left is a BigInt beside a shape whose
    // ToPrimitive is user code: `({[Symbol.toPrimitive]() { throw }})
    // >>> 0n` must raise what that code raises, and only the any
    // kernel runs it in the right order. The lanes above route every
    // such pair, so this is the message for what none of them
    // claimed rather than a decision taken ahead of them.
    if l == Type::BigInt || r == Type::BigInt {
        return Err("`>>>` is not defined on BigInt operands per spec".into());
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
