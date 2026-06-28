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

#![allow(clippy::too_many_arguments)]

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, op: AstBinOp, a: Operand, b: Operand) -> Operand {
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
    if let Some(v) = crate::ssa_lower_binop_inner_any_arith::try_lower(ctx, op, a, b) {
        return v;
    }
    // Strict eq (=== / !==) path — see
    // [`crate::ssa_lower_binop_inner_strict_eq`] (chunk 189 —
    // sub-stage extraction mirroring chunks 185-188).
    if let Some(v) = crate::ssa_lower_binop_inner_strict_eq::try_lower(ctx, op, a, b) {
        return v;
    }
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
    let (a, b) = if coerce_op {
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
    };
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
    if matches!(op, AstBinOp::Add) {
        // S142 — String + Undefined per ES §13.15.3. Undefined lowers
        // to ConstPtrNull (same i64-0 ABI as Null), so the bool/null
        // detection downstream can't distinguish the two from operand
        // shape alone. Resolve the Undefined side here via the
        // `binop_*_undef_id` hint set by lower_binop_with_ids; emit
        // `__torajs_undefined_to_str()` and replace the operand with
        // the resulting Str so the str+str fast path picks it up.
        // Guard on the *other* side being string-shaped so numeric
        // `undefined + 0` (spec: NaN) keeps its current behavior.
        let mut a = a;
        let mut b = b;
        let str_shaped = |t: Type| matches!(t, Type::Str | Type::Substr);
        if ctx.binop_left_undef_id.is_some() && str_shaped(ctx.operand_ty(&b)) {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.undefined_to_str, vec![]),
                Type::Str,
                None,
            );
            a = Operand::Value(v);
        }
        if ctx.binop_right_undef_id.is_some() && str_shaped(ctx.operand_ty(&a)) {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.undefined_to_str, vec![]),
                Type::Str,
                None,
            );
            b = Operand::Value(v);
        }
        let a_ty = ctx.operand_ty(&a);
        let b_ty = ctx.operand_ty(&b);
        // V3-18 m1.d / m3.c — string concat with Bool / Null /
        // BigInt on either side. ssa_lower coerces via
        // __torajs_bool_to_str / __torajs_null_to_str /
        // __torajs_bigint_to_string before concat.
        let bool_or_null = |t: Type, op: &Operand| -> bool {
            matches!(t, Type::Bool) || matches!(op, Operand::ConstPtrNull)
        };
        let str_or_substr = |t: Type| matches!(t, Type::Str | Type::Substr);
        // S138 — `String + Arr` / `String + Obj` (ES §13.15.3
        // ToPrimitive(Default) → ToString on the non-String side).
        // Mirror of the explicit `String(arr) / String(struct)`
        // S137 coerce — routes Arr through arr_join(",") and Obj
        // through the `"[object Object]"` literal.
        let arr_or_obj = |t: Type| matches!(t, Type::Arr(_) | Type::Obj(_));
        let mixed_string = matches!(
            (a_ty, b_ty),
            (Type::Str, Type::I64)
                | (Type::Str, Type::F64)
                | (Type::Str, Type::BigInt)
                | (Type::I64, Type::Str)
                | (Type::F64, Type::Str)
                | (Type::BigInt, Type::Str)
                | (Type::Substr, Type::I64)
                | (Type::Substr, Type::F64)
                | (Type::Substr, Type::BigInt)
                | (Type::I64, Type::Substr)
                | (Type::F64, Type::Substr)
                | (Type::BigInt, Type::Substr)
        ) || (str_or_substr(a_ty) && bool_or_null(b_ty, &b))
            || (str_or_substr(b_ty) && bool_or_null(a_ty, &a))
            || (str_or_substr(a_ty) && arr_or_obj(b_ty))
            || (str_or_substr(b_ty) && arr_or_obj(a_ty));
        // Any Substr operand: route through view-aware concat
        // helpers. One alloc + two memcpys (vs. 2 allocs + 3
        // memcpys via substr_to_owned + str_concat).
        let either_substr = a_ty == Type::Substr || b_ty == Type::Substr;
        if either_substr
            && (a_ty == Type::Str || a_ty == Type::Substr)
            && (b_ty == Type::Str || b_ty == Type::Substr)
        {
            let target = match (a_ty, b_ty) {
                (Type::Substr, Type::Str) => ctx.intrinsics.substr_concat_substr_str,
                (Type::Str, Type::Substr) => ctx.intrinsics.substr_concat_str_substr,
                (Type::Substr, Type::Substr) => ctx.intrinsics.substr_concat_substr_substr,
                _ => unreachable!(),
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(target, vec![a, b]),
                Type::Str,
                None,
            );
            return Operand::Value(v);
        }
        if a_ty == Type::Str && b_ty == Type::Str {
            let concat = ctx.intrinsics.str_concat;
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(concat, vec![a, b]),
                Type::Str,
                None,
            );
            return Operand::Value(v);
        }
        if mixed_string {
            let coerce = |ctx: &mut LowerCtx, v: Operand| -> Operand {
                match ctx.operand_ty(&v) {
                    Type::Str => v,
                    Type::Substr => {
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::I64 => {
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.i64_to_str, vec![v]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::F64 => {
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.f64_to_str, vec![v]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::Bool => {
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.bool_to_str, vec![v]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::BigInt => {
                        // V3-18 m3.c — BigInt → String concat. The
                        // BigInt is consumed by bigint_to_string
                        // (rc-managed; helper handles the inc).
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.bigint_to_string, vec![v]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::Ptr if matches!(v, Operand::ConstPtrNull) => {
                        // V3-18 m1.d — null literal → "null".
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    // S138 — Arr / Obj sides reuse the S137 dispatch.
                    Type::Arr(elem_arr_id) => {
                        let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
                        let join_fid = match elem_ty {
                            Type::Substr => ctx.intrinsics.arr_join_substr,
                            Type::I64 => ctx.intrinsics.arr_join_i64,
                            Type::F64 => ctx.intrinsics.arr_join_f64,
                            Type::Bool => ctx.intrinsics.arr_join_bool,
                            Type::Any => ctx.intrinsics.arr_join_any,
                            _ => ctx.intrinsics.arr_join,
                        };
                        let sep = ctx.intern_string_literal(",");
                        let r = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(join_fid, vec![v, Operand::Value(sep)]),
                            Type::Str,
                            None,
                        );
                        Operand::Value(r)
                    }
                    Type::Obj(_) => Operand::Value(ctx.intern_string_literal("[object Object]")),
                    other => panic!("ssa-lower: mixed string concat unexpected type {other:?}"),
                }
            };
            let a_str = coerce(ctx, a);
            let b_str = coerce(ctx, b);
            let concat = ctx.intrinsics.str_concat;
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(concat, vec![a_str, b_str]),
                Type::Str,
                None,
            );
            return Operand::Value(v);
        }
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
            _ => !ctx.binop_mul_square,
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
