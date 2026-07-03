//! `+` string-concat family of `lower_binop_inner` — Undefined-
//! side resolution, Substr view-aware concat, Str×Str concat, and
//! the mixed Number/Bool/Null/BigInt/Arr/Obj ToString-then-concat
//! path. Split from `ssa_lower_binop_inner.rs` (2026-07-03,
//! fn-debt decomp) as a `try_lower` sibling mirroring the
//! `binop_inner_{any_arith,strict_eq,bigint,str_cmp,f64,i64}`
//! family. Bodies verbatim; mechanical rewrites: matched-path
//! `return Operand::Value(v)` → `Some(..)`, the fall-through tail
//! becomes `None`, and the `coerce` closure hoists to the
//! file-local [`coerce_to_str`] fn.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Add) {
        return None;
    }
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
        return Some(Operand::Value(v));
    }
    if a_ty == Type::Str && b_ty == Type::Str {
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a, b]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    if mixed_string {
        let a_str = coerce_to_str(ctx, a);
        let b_str = coerce_to_str(ctx, b);
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a_str, b_str]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}

/// One operand → owned Str for the mixed-concat path (body is the
/// pre-split `coerce` closure, verbatim).
fn coerce_to_str(ctx: &mut LowerCtx, v: Operand) -> Operand {
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
}
