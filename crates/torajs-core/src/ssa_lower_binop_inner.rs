//! `lower_binop_inner` extracted from [`crate::ssa_lower`]
//! (chunk 160).
//!
//! Pre-extract this method was 902 LOC on `LowerCtx` — the single
//! biggest god-fn left in ssa_lower.rs after chunks 145-159 wrapped
//! up lower_stmt / lower_for_of_str / lower_json_stringify /
//! emit_drop_value. Body verbatim moves here as a free fn;
//! `LowerCtx::lower_binop_inner` becomes a thin private wrapper.
//!
//! Sibling is **over the 500-LOC file-size HARD limit** (~895 LOC)
//! and is registered in `torajs-file-size-debt.md` as known-debt.
//! The decomposition into per-operator-family sub-siblings
//! (arith / cmp / logical-fold / bitwise / string-concat-coerce /
//! js-add-coerce / etc) is deferred to a follow-up rotation —
//! this chunk just gets the LOC out of ssa_lower.rs so the rest
//! of the god-file decomposition can continue.
//!
//! Body unchanged — every JS operator dispatch (+ - * / %, ===,
//! ==, < <= > >=, & | ^ << >> >>>, &&, ||, ??, in, instanceof, etc)
//! preserved verbatim including all V3-18 m* coercion paths,
//! ssa_lower_binop_loose_eq / ssa_lower_binop_null_undef try-lower
//! short-circuits, Any-tagged dispatch, str_concat, BigInt arith,
//! pointer-cmp fallbacks.

//!
//! 2026-07-03 fn-debt decomp: the `+` string-concat family →
//! [`add_str`] (dir submodule, try_lower sibling); the Bool/Null
//! ToNumber coercion block → file-local
//! [`coerce_bool_null_operands`].

#![allow(clippy::too_many_arguments)]

pub(crate) mod add_str;

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, op: AstBinOp, a: Operand, b: Operand) -> Operand {
    // V3-18 m3 — `==` / `!=` short-circuit folds for null /
    // (Any, null) / (String, Number) / (Bool, String) cross-
    // type pairs per spec §7.2.13. See
    // [`crate::ssa_lower_binop_loose_eq::try_lower`].
    if let Some(v) = crate::ssa_lower_binop_loose_eq::try_lower(ctx, op, a, b) {
        return v;
    }
    // ES §7.2.15 — `null === undefined` static fold via
    // binop_*_undef_id flags. See
    // [`crate::ssa_lower_binop_null_undef::try_lower`].
    if let Some(v) = crate::ssa_lower_binop_null_undef::try_lower(ctx, op, a, b) {
        return v;
    }
    // V3-18 m3.b — `===` / `!==` cross-type: when the runtime
    // types differ, spec §7.2.15 returns false unconditionally
    // (no throw). Static-fold to ConstBool here so the
    // downstream same-type cmp path doesn't see mismatched ops.
    // Per spec, Number and Boolean are DIFFERENT JS types
    // (`1 === true` is false), so they can't share a family
    // even though both lower to integer-shaped operands.
    //
    // Pointer-shaped types (Obj/Arr/Closure/Symbol/Promise/...)
    // share a family because a Nullable<T> can carry null AND
    // any heap pointer; the existing pointer-cmp path handles
    // both correctly. Without this carve-out, `obj.next === null`
    // would static-false even when obj.next IS null at runtime.
    // P0.6 / P0.7 / P0.8 — Any-aware BinOp dispatch for
    // {Add, Sub, Mul, Div, Mod, Lt, Le, Gt, Ge} when at
    // least one operand is Type::Any. See
    // [`crate::ssa_lower_binop_inner_any_arith`] (chunk 190 —
    // sub-stage extraction mirroring chunks 185-189; gets
    // this file ≤500 LOC HARD).
    // An object operand on an operator whose ToPrimitive can answer
    // something other than a number. Boxing it hands the any-lane
    // kernels below the shape they dispatch on, so the object case and
    // the `any` case become one path — and those kernels already carry
    // the whole §13.15.3 / §13.10 / §13.12 walk (valueOf then
    // toString, either answer, throws propagated).
    //
    // `-` / `*` / `/` / `%` deliberately do NOT come here: ToNumeric
    // has no answer but a number, so they keep the static f64 lane in
    // `coerce_object_operands` below.
    let (a, b) = box_object_operands_for_any_lane(ctx, op, a, b);
    if let Some(v) = crate::ssa_lower_binop_inner_any_arith::try_lower(ctx, op, a, b) {
        return v;
    }
    // Strict eq (=== / !==) path — see
    // [`crate::ssa_lower_binop_inner_strict_eq`] (chunk 189 —
    // sub-stage extraction mirroring chunks 185-188).
    if let Some(v) = crate::ssa_lower_binop_inner_strict_eq::try_lower(ctx, op, a, b) {
        return v;
    }
    let (a, b) = coerce_object_operands(ctx, op, a, b);
    let (a, b) = coerce_bool_null_operands(ctx, op, a, b);
    // T-25 BigInt × BigInt path — see
    // [`crate::ssa_lower_binop_inner_bigint`] (chunk 187 —
    // sub-stage extraction mirroring chunks 185/186).
    if let Some(v) = crate::ssa_lower_binop_inner_bigint::try_lower(ctx, op, a, b) {
        return v;
    }
    // String concat short-circuit. Routes `str + str` to the runtime
    // concat intrinsic, which takes ownership of both operands.
    // Mixed Number+String / String+Number coerce the number to its
    // decimal string form first via the runtime, then concat —
    // matches JS spec ToString behavior.
    if let Some(v) = add_str::try_lower(ctx, op, a, b) {
        return v;
    }
    // String content-equality. ECMA-262 §7.2.16: `===` on strings
    // is bytes-equal, not pointer-equal. Without this dispatch,
    // two identical literals in different alloc sites produce
    // !==. test262-port spike caught this. !== is content-equality
    // negated.
    //
    // Substr operand support: `Substr === Str` and `Str === Substr`
    // route through `substr_eq_str` (substr always on left). Two
    // Substr operands materialize lhs to OWNED first (rare path —
    // no current bench / conformance triggers it).
    // Str × Str eq + ordered-compare path — see
    // [`crate::ssa_lower_binop_inner_str_cmp`] (chunk 188 —
    // sub-stage extraction mirroring chunks 185/186/187).
    if let Some(v) = crate::ssa_lower_binop_inner_str_cmp::try_lower(ctx, op, a, b) {
        return v;
    }

    // V3-01 — `**` for Number lowers via libm `pow`, which always
    // takes + returns f64. Force both operands into the float
    // path so downstream consumers see a Number-shaped result.
    //
    // W3 (rfc 20260611-ann-width-unification §5.3) — `%` can mint
    // -0 at runtime (negative dividend with zero remainder, JS
    // spec §13.10), which i64 cannot represent, so Mod also forces
    // the float path. A provably non-negative constant dividend
    // keeps the int path: a ≥ 0 bounds the remainder in [+0, |b|).
    // The egraph truncation-recovery rewrites and float_demote
    // narrow the -0-insensitive / interval-proven shapes back to
    // srem downstream.
    //
    // srem runtime-0 (§5.3 follow-up close) — the carve must ALSO
    // prove the divisor non-zero: `7 % b` with b == 0 at runtime
    // is NaN per spec, but aarch64 sdiv-by-zero yields 0 and the
    // msub hands the dividend back (silent 7). A non-zero
    // constant divisor is the provable case; everything else
    // floats and lets frem mint the NaN.
    let mod_int_safe =
        matches!(a, Operand::ConstI64(c) if c >= 0) && matches!(b, Operand::ConstI64(d) if d != 0);
    // W3 C4 + S9 — int `a * b` mints -0 only when one factor is
    // zero and the other negative. A positive constant cofactor
    // rules that out and keeps the int path, as do two constants
    // that miss the zero×negative pattern and a square (`x * x`,
    // the with_ids side channel); everything else — non-positive
    // constant cofactor (`x * -1`, `0 * x`) and variable×variable
    // (S9, runtime zero × runtime negative) — floats. The
    // frem_narrow trunc-tree recovery (chunk ②) pulls the
    // -0-insensitive i64-sink shapes back to int.
    let mul_minus_zero_risk = matches!(op, AstBinOp::Mul)
        && match (&a, &b) {
            (Operand::ConstI64(x), Operand::ConstI64(y)) => {
                (*x == 0 && *y < 0) || (*x < 0 && *y == 0)
            }
            (Operand::ConstI64(c), _) | (_, Operand::ConstI64(c)) => *c <= 0,
            _ => !ctx.binop.mul_square,
        };
    let force_float = matches!(op, AstBinOp::Div | AstBinOp::Pow)
        || (matches!(op, AstBinOp::Mod) && !mod_int_safe)
        || mul_minus_zero_risk;
    let either_float = ctx.operand_ty(&a) == Type::F64 || ctx.operand_ty(&b) == Type::F64;
    let is_float = force_float || either_float;

    if is_float {
        // f64 path — see [`crate::ssa_lower_binop_inner_f64`]
        // (chunk 186 — sub-stage extraction mirroring chunk 185).
        return crate::ssa_lower_binop_inner_f64::lower_float_path(ctx, op, a, b);
    }

    // i64 fall-through path — see [`crate::ssa_lower_binop_inner_i64`]
    // (chunk 185 — sub-stage extraction mirroring chunks 179-184).
    crate::ssa_lower_binop_inner_i64::lower_i64_default(ctx, op, a, b)
}

/* V3-18 m1.a — JS spec §13.15.3 ToNumber coercion for `+`
 * with Boolean / Null operands. Both sides become i64
 * before the actual add; the existing i64-add path then
 * handles them as plain integers.
 *
 * Coercion table:
 *   Bool   → zext (false=0, true=1)
 *   Null   → const 0 (Type::Ptr operand replaced with i64 0)
 *   Number → already i64 (when typed `number` defaults to i64)
 *   F64 / String / BigInt / Substr → not in this path, fall
 *   through to existing handlers.
 *
 * check.rs's `js_add_coerces_to_number` gates which (l, r)
 * combos hit this branch — only Number/Boolean/Null pairs
 * with at least one non-Number side. Pure Number+Number
 * stays on the existing path. */
/// §13.7-§13.10 — `-` / `*` / `/` / `%` call ToNumeric on each
/// operand unconditionally, so an object side runs its `valueOf`
/// (and answers NaN when it has none). `+` is deliberately NOT here:
/// its ToPrimitive uses the DEFAULT hint, whose answer for a
/// hook-free object is a STRING, so `{v:1} + 1` is "[object Object]1"
/// rather than NaN — a dynamic result type this static lane cannot
/// promise.
///
/// The kernel is the one the `any` lane already used. Boxing the
/// pointer is a pure encode with no refcount traffic, and the throw
/// check is there because a user `valueOf` can throw.
/// Box an object operand so the any-lane kernels claim the expression.
///
/// The checker types these pairs exactly as it types an `any` operand
/// (`objectish_numeric_pair` in `check_type_of_binop`), so the value
/// the kernel answers with is the one the surrounding code expects.
///
/// A String on the other side is left alone — EXCEPT under the
/// bitwise family, where §13.12 ToNumber's it and the String side
/// itself boxes (rotation 546): `Str + obj` has its own concat lane
/// that the checker types String, and boxing here would answer Any
/// instead. The ordering operators never reach this with a String
/// either — the checker refuses that pair outright.
fn box_object_operands_for_any_lane(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> (Operand, Operand) {
    if !matches!(
        op,
        AstBinOp::Add
            | AstBinOp::Pow
            | AstBinOp::Lt
            | AstBinOp::Le
            | AstBinOp::Gt
            | AstBinOp::Ge
            | AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr
    ) {
        return (a, b);
    }
    // §13.12 — the bitwise family ToNumber's a String operand like
    // any object side (`"3" & 1` is `1`), so a Str/Substr side boxes
    // into the any lane too (rotation 546, the 542-02 ledger entry).
    // Everywhere else a Str side must NOT box: `+` has its concat
    // lanes and the ordering operators own §13.10's string branch —
    // that is what the escape below protects.
    let bitwise = matches!(
        op,
        AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr
    );
    let stringish = |t: &Type| matches!(t, Type::Str | Type::Substr);
    let coercible = |ctx: &mut LowerCtx, v: &Operand| {
        let t = ctx.operand_ty(v);
        crate::ssa_lower_unary::is_number_coercible_obj(t) || (bitwise && stringish(&t))
    };
    let a_obj = coercible(ctx, &a);
    let b_obj = coercible(ctx, &b);
    if !a_obj && !b_obj {
        return (a, b);
    }
    if !bitwise && (stringish(&ctx.operand_ty(&a)) || stringish(&ctx.operand_ty(&b))) {
        return (a, b);
    }
    let a = if a_obj { ctx.box_to_any(a) } else { a };
    let b = if b_obj { ctx.box_to_any(b) } else { b };
    (a, b)
}

fn coerce_object_operands(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> (Operand, Operand) {
    if !matches!(
        op,
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod
    ) {
        return (a, b);
    }
    let a_obj = crate::ssa_lower_unary::is_number_coercible_obj(ctx.operand_ty(&a));
    let b_obj = crate::ssa_lower_unary::is_number_coercible_obj(ctx.operand_ty(&b));
    if !a_obj && !b_obj {
        return (a, b);
    }
    // Both sides land on F64: the object side has to, and leaving the
    // other on I64 would put a mixed pair in front of the numeric
    // lanes that expect one width.
    let a = coerce_operand_to_f64(ctx, a, a_obj);
    let b = coerce_operand_to_f64(ctx, b, b_obj);
    (a, b)
}

fn coerce_operand_to_f64(ctx: &mut LowerCtx, v: Operand, is_obj: bool) -> Operand {
    if is_obj {
        let boxed = ctx.box_to_any(v.clone());
        let n = ctx.coerce_any_to_number(boxed, Type::F64);
        ctx.emit_throw_check(None);
        return n;
    }
    if matches!(ctx.operand_ty(&v), Type::I64) {
        return ctx.coerce_to_f64(v);
    }
    v
}

fn coerce_bool_null_operands(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> (Operand, Operand) {
    let coerce_op = matches!(
        op,
        AstBinOp::Add
            | AstBinOp::Sub
            | AstBinOp::Mul
            | AstBinOp::Div
            | AstBinOp::Mod
            | AstBinOp::Lt
            | AstBinOp::Gt
            | AstBinOp::Le
            | AstBinOp::Ge
            | AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr
            | AstBinOp::LooseEq
            | AstBinOp::LooseNeq
    );
    if coerce_op {
        let a_ty = ctx.operand_ty(&a);
        let b_ty = ctx.operand_ty(&b);
        let a_is_null = matches!(a, Operand::ConstPtrNull);
        let b_is_null = matches!(b, Operand::ConstPtrNull);
        // A side is coercible-to-Number iff it's already i64,
        // a bool (zext to i64), or the null literal (const 0).
        let a_coerce = matches!(a_ty, Type::I64 | Type::Bool) || a_is_null;
        let b_coerce = matches!(b_ty, Type::I64 | Type::Bool) || b_is_null;
        // Trigger only when at least one side is non-Number — pure
        // Number+Number stays on the existing fast path.
        let either_bool_or_null =
            matches!(a_ty, Type::Bool) || matches!(b_ty, Type::Bool) || a_is_null || b_is_null;
        if either_bool_or_null && a_coerce && b_coerce {
            let a2 = if a_is_null {
                Operand::ConstI64(0)
            } else if matches!(a_ty, Type::Bool) {
                ctx.coerce_bool_to_i64(a)
            } else {
                a
            };
            let b2 = if b_is_null {
                Operand::ConstI64(0)
            } else if matches!(b_ty, Type::Bool) {
                ctx.coerce_bool_to_i64(b)
            } else {
                b
            };
            (a2, b2)
        } else {
            (a, b)
        }
    } else {
        (a, b)
    }
}
