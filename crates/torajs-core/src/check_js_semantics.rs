//! JS-spec semantics helpers pulled out of [`crate::check`] as chunk-321
//! of the check.rs god-file decomp.
//!
//! Five pure spec-rule predicates (no `Checker` state):
//!
//! - `js_add_coerces_to_number` — V3-18 m1.a, §13.15.3.
//! - `js_arith_coerces_to_number` — V3-18 m1.b, §13.7-§13.10.
//! - `is_known_builtin_global` — V3-18 m1.h.3 built-in JS globals.
//! - `js_truthy_acceptable` — V3-18 m1.h.1, §7.1.2 ToBoolean.
//! - `js_loose_eq_supported` — V3-18 m3, §7.2.13 IsLooselyEqual.
//!
//! Grouped here as the "JS semantics acceptance" family. Re-exported
//! back into `crate::check` so callers (`check_type_of_binop`,
//! `check_type_of_ternary`, `check_type_of_unary`, `check_type_of_misc`,
//! `check_stmt_while`, …) keep `crate::check::<name>` import paths.

use crate::check::Type;

/// True when a value of this type sits in a NaN-boxed `any` slot, so
/// every operator family's `any` arm is the arm that describes it.
///
/// `Type::Any` is the obvious member. The other is a `Nullable<T>`
/// over a scalar: `number` and `boolean` have no bit pattern to
/// spare for `undefined` the way a pointer has its in-band 0 or a
/// string has its oddball, so `ssa_lower_parse_type::
/// nullable_inner_boxes` hands them an `any` slot instead. These two
/// are mirrors and must move together — a scalar that starts or
/// stops boxing there belongs on the other side here the same day.
///
/// What it buys is the answer §13.15.3 and §13.7-§13.12 already give:
/// `undefined + 1` is `NaN`, and reaching it needs no new lowering,
/// because the operand is already boxed and the any-lane kernels have
/// carried that ToNumeric / ToPrimitive walk since the first `any`
/// operand reached them. A runtime refusing to run the file was never
/// one of the answers; TypeScript's own is a definite-assignment
/// diagnostic, which is a different question (567-04).
///
/// The result stays `Any` because the kernel's answer is a boxed
/// value. A `Nullable<string>` is deliberately NOT a member: its slot
/// is a real `str` whose null is an in-band 0, so the any lane would
/// be handed something it cannot read.
pub(crate) fn rides_any_lane(t: &Type) -> bool {
    match t {
        Type::Any => true,
        Type::Nullable(inner) => matches!(**inner, Type::Number | Type::Boolean),
        _ => false,
    }
}

/// V3-18 m1.a — JS spec §13.15.3 ApplyStringOrNumericBinaryOperator
/// guard for non-string `+`. Returns true iff both operands are
/// statically-typed numerics-or-coercible-to-numerics: Number,
/// Boolean (ToNumber → 0/1), Null (ToNumber → 0). When this holds,
/// check.rs accepts `l + r` with result type Number; ssa_lower
/// mirrors the coercion at lower time. Strings are handled by the
/// existing String+String / String+Number arms (they short-circuit
/// before this helper). Excludes BigInt — mixed BigInt+Number is a
/// TypeError per spec, caught by the trailing catch-all.
pub(crate) fn js_add_coerces_to_number(l: &Type, r: &Type) -> bool {
    fn coerces(t: &Type) -> bool {
        matches!(t, Type::Number | Type::Boolean | Type::Null)
    }
    coerces(l) && coerces(r) && (*l != Type::Number || *r != Type::Number)
}

/// V3-18 m1.b — `-` / `*` / `/` / `%` use the same ToNumber rule
/// as `+` but never have a String-concat path (spec §13.7-§13.10
/// unconditionally call ToNumeric). Same coercibles as m1.a.
pub(crate) fn js_arith_coerces_to_number(l: &Type, r: &Type) -> bool {
    js_add_coerces_to_number(l, r)
}

/// V3-18 m1.h.3 — built-in JS globals that tora's check.rs
/// resolves via the special Ident → Type::Object("X") arms.
/// Used by the `typeof undeclared` path to avoid mis-classifying
/// a tora built-in as "undefined".
pub(crate) fn is_known_builtin_global(name: &str) -> bool {
    matches!(
        name,
        "console"
            | "Math"
            | "Object"
            | "Number"
            | "String"
            | "JSON"
            | "Array"
            | "Date"
            | "WeakRef"
            | "WeakMap"
            | "WeakSet"
            | "Symbol"
            | "BigInt"
            | "Boolean"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "EvalError"
            | "URIError"
            | "Bun"
            | "Promise"
            | "RegExp"
            | "Iterator"
            | "Map"
            | "Set"
            | "fetch"
            | "process"
            | "globalThis"
            | "undefined"
            | "NaN"
            | "Infinity"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "parseInt"
            | "parseFloat"
            | "isFinite"
            | "isNaN"
            | "Function"
            | "eval"
            // WHATWG HTML microtask scheduling (P10.1)
            | "queueMicrotask"
    )
}

/// V3-18 m1.h.1 — JS spec §7.1.2 ToBoolean acceptance check.
/// Used at every condition site (if / while / do-while / for /
/// ternary). Anything except Void is coercible to bool — Number
/// (0/NaN→false), String (""→false), Null (→false), Object/etc
/// (always-true). ssa_lower routes the cond through
/// `coerce_to_bool` to emit the actual coercion at lower time.
pub(crate) fn js_truthy_acceptable(t: &Type) -> bool {
    !matches!(t, Type::Void)
}

/// V3-18 m3 — `==` / `!=` cross-type pair guard. Spec §7.2.13:
///   Number == Boolean → coerce Boolean
///   Boolean == Number → coerce Boolean
///   Number == Null    → false  (not coerced)
///   Null   == Boolean → false
///   ...
/// We accept any pair of Number/Boolean/Null types — the runtime
/// coercion (or static-false for null vs non-null-non-undefined)
/// happens at lower time. String / BigInt / Object cross-type
/// pairs go through ToPrimitive → numeric and ship in a later
/// wedge.
pub(crate) fn js_loose_eq_supported(l: &Type, r: &Type) -> bool {
    // RFC 20260713-loose-eq-substrate blade 1 — either side Any:
    // the runtime ladder (`__torajs_anyv_loose_eq`) resolves every
    // §7.2.14 tag pair at runtime. Void never holds a value and a
    // fn-value operand lowers to a raw fn address the Any boxer
    // has no encoding for (`fn == primitive` stays a checker
    // reject — recorded RFC residual), so those two stay out.
    if matches!(l, Type::Any) || matches!(r, Type::Any) {
        let ladder_ok = |t: &Type| !matches!(t, Type::Void | Type::Function(..) | Type::Rest(..));
        return ladder_ok(l) && ladder_ok(r);
    }
    if matches!(l, Type::Number | Type::Boolean | Type::Null)
        && matches!(r, Type::Number | Type::Boolean | Type::Null)
    {
        return true;
    }
    // RFC 20260713-loose-eq-substrate blade 2 — BigInt loose-eq
    // mixes (§7.2.14 steps 7-8 / 12-13): BigInt × {Number, String,
    // Boolean} route through the runtime ladder (StringToBigInt /
    // exact BigInt-vs-f64 mathematical compare — never rounded
    // through f64).
    let bigint_mix = |t: &Type| matches!(t, Type::Number | Type::String | Type::Boolean);
    if (matches!(l, Type::BigInt) && bigint_mix(r)) || (matches!(r, Type::BigInt) && bigint_mix(l))
    {
        return true;
    }
    // ES §7.2.13 steps 2-4 — Null / Undefined never coerce to other
    // primitive types for loose-eq, so the cross-type result is
    // statically false (unless the other side is also nullish, in
    // which case the result is true). Accept Undefined symmetric with
    // Null in the same nullish bucket so the type-check passes; SSA
    // lowering's `a_is_null || b_is_null` arm (ssa_lower.rs L25533)
    // already folds to `ConstBool(false)` for cross-type and
    // `ConstBool(true)` for nullish-nullish — same fold both literals
    // hit since they share `Operand::ConstPtrNull`. Other side may be
    // any primitive (Number / Boolean / String / BigInt) — the fold
    // doesn't need to inspect it.
    let nullish = |t: &Type| matches!(t, Type::Null | Type::Undefined);
    let primitive = |t: &Type| {
        matches!(
            t,
            Type::Number
                | Type::Boolean
                | Type::Null
                | Type::Undefined
                | Type::String
                | Type::BigInt
        )
    };
    if (nullish(l) && primitive(r)) || (nullish(r) && primitive(l)) {
        return true;
    }
    // Chunk 618 — nullish × heap-reference (`arr == null`, `obj !=
    // null`, ...) is everyday JS null-probing (§7.2.13 steps 2-3:
    // an object never loose-equals nullish, but the VALUE may be
    // nullish — a Nullable narrow can leak a null into a bare
    // heap-typed binding, p2 probe). ssa_lower emits a runtime
    // `ptr == null` icmp, correct in both cases.
    let heap_ref = |t: &Type| {
        matches!(
            t,
            Type::Array(_)
                | Type::Struct(_)
                | Type::Object(_)
                | Type::ClassRef(_)
                | Type::Map
                | Type::Set
                | Type::MapIter
                | Type::ArrIter
                | Type::Date
                | Type::RegExp
                | Type::Symbol
                | Type::Promise(_)
                | Type::WeakRef
                | Type::WeakMap
                | Type::WeakSet
        )
    };
    if (nullish(l) && heap_ref(r)) || (nullish(r) && heap_ref(l)) {
        return true;
    }
    // RFC 20260713-loose-eq-substrate blade 3 — C3: a same-type
    // object pair (§7.2.14 step 1 → IsStrictlyEqual) is pointer
    // identity, mirroring `===`.
    if heap_ref(l) && l == r {
        return true;
    }
    // Blade 3 — C4: object × primitive coerces the object through
    // ToPrimitive (§7.2.14 steps 11-12) via the runtime ladder
    // (user valueOf/toString dispatch included).
    let to_prim_mix = |t: &Type| {
        matches!(
            t,
            Type::Number | Type::String | Type::Boolean | Type::BigInt
        )
    };
    if (heap_ref(l) && to_prim_mix(r)) || (heap_ref(r) && to_prim_mix(l)) {
        return true;
    }
    // Chunk 619 — nullish × Any (`u == a` with u: undefined,
    // a: any): ssa_lower's S127-2 arm already emits the
    // `any_strict_eq(NULL) || any_strict_eq(UNDEF)` probe for a
    // nullish-typed side (612 wired the type-based flags); only
    // this admit was missing. Strict === has accepted the pair
    // since m3.b.
    if (nullish(l) && matches!(r, Type::Any)) || (nullish(r) && matches!(l, Type::Any)) {
        return true;
    }
    // ES §7.2.13 step 6 — String <-> Number cross-pair coerces the
    // string side via ToNumber + numeric compare. ssa_lower
    // (loose-eq arm following the nullish fold) emits
    // `__torajs_str_to_number` + fcmp.
    if (matches!(l, Type::String) && matches!(r, Type::Number))
        || (matches!(l, Type::Number) && matches!(r, Type::String))
    {
        return true;
    }
    // ES §7.2.13 steps 7-9 — Boolean <-> String coerces both sides
    // to Number (Boolean → 0/1, String → ToNumber). ssa_lower
    // (loose-eq arm sibling to the Str/Number arm) emits the
    // compound coercion + fcmp.
    if (matches!(l, Type::String) && matches!(r, Type::Boolean))
        || (matches!(l, Type::Boolean) && matches!(r, Type::String))
    {
        return true;
    }
    // S127-2 — Any vs Null (covers both `null` and `undefined` literals,
    // which both lower to Type::Null at check time). spec §7.2.13
    // IsLooselyEqual: `v == null` is true iff `v` is null or undefined,
    // for any runtime type of `v`. ssa_lower folds this into a nullish
    // check via `any_strict_eq_imm_pair` against tag=0 (ANY_NULL) and
    // tag=5 (ANY_UNDEF). The standard idiom `v == null` is widespread in
    // callbacks over Array<Any> (.some / .every / .find / .reduce).
    matches!((l, r), (Type::Any, Type::Null) | (Type::Null, Type::Any))
}
