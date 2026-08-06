//! Strict equality (`===` / `!==`) BinOp dispatch extracted
//! from [`crate::ssa_lower_binop_inner::lower`] (chunk 189 —
//! continuation of file-size debt cleanup, mirrors chunks
//! 185/186/187/188 shape).
//!
//! Two sub-arms inside the `op ∈ {Eq, Neq}` gate:
//!
//! - **Any === path** (P0.3) — when either operand is
//!   `Type::Any`, route through runtime helpers. Both Any →
//!   `any_any_strict_eq(a, b)`. One Any + one concrete → pack
//!   the concrete side as `(tag, value-as-i64)` and call
//!   `any_strict_eq(any_box, tag, value)`, mirroring the
//!   box_to_any tag/value extraction. P1.8 — `any === undefined`
//!   vs `any === null` must carry distinct tags (5 vs 0) so the
//!   tag-equality short-circuit fires correctly. `Neq` Xor-
//!   negates the bool result.
//!
//! - **Cross-type fold** — when no operand is Any, classify
//!   each as numeric (I64/F64) or pointer-ish (Ptr/Str/Substr/
//!   Obj/Arr/Closure/Symbol/Promise/RegExp/Date/WeakRef/
//!   WeakMap/WeakSet/BigInt/Any). If the two operands fall in
//!   different families AND aren't the same Type, fold to a
//!   constant `false` (Eq) or `true` (Neq) — JS spec says
//!   strict-eq across distinct runtime types is always false.
//!   Same-family pairs fall through to the f64 / i64 path
//!   for real cmp.
//!
//! Returns `Some(Operand)` on Any-path hit or cross-type fold
//! match, `None` when op outside {Eq, Neq} OR same-family fall-
//! through. Caller falls through unchanged on `None`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        return None;
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    // P0.3 — Any === / !== per JS spec §7.2.13. When either
    // operand is Type::Any the static-shape compare can't
    // resolve at the SSA layer; route through runtime helpers
    // that unbox each side and compare tag-then-payload.
    if matches!(a_ty, Type::Any) || matches!(b_ty, Type::Any) {
        let result = if matches!(a_ty, Type::Any) && matches!(b_ty, Type::Any) {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_any_strict_eq, vec![a, b]),
                Type::Bool,
                None,
            );
            Operand::Value(r)
        } else {
            // Pack the concrete side as (tag, value-as-i64) so
            // the helper avoids a fresh Any-box alloc per
            // compare. Mirrors the box_to_any tag/value
            // extraction.
            let (any_box, concrete, concrete_ty, concrete_is_undef, concrete_is_null) =
                if matches!(a_ty, Type::Any) {
                    (
                        a,
                        b,
                        b_ty,
                        ctx.binop.right_undef_id.is_some(),
                        ctx.binop.right_null_id.is_some(),
                    )
                } else {
                    (
                        b,
                        a,
                        a_ty,
                        ctx.binop.left_undef_id.is_some(),
                        ctx.binop.left_null_id.is_some(),
                    )
                };
            // P1.8 + 2026-07-16 — `any === undefined` and `any === null`
            // are distinct: the concrete side must carry the matching
            // tag (5 vs 0) so the runtime helper's tag-equality short-
            // circuit fires correctly. Pre-P1.8 both Ptr-shaped
            // operands packed to 0, making `<undefined-box> ===
            // undefined` falsely false and `<undefined-box> === null`
            // falsely true. P1.8 fixed the literal `undefined` / `null`
            // path (only when the SSA operand was `Type::Ptr` +
            // `Operand::ConstPtrNull`), but a checker-typed
            // `Type::Undefined` / `Type::Null` binding (e.g.
            // `const y = undefined; anyBox === y`) reached here as a
            // `Load` operand (not `ConstPtrNull`) and fell through to
            // `_ => (0, 0)` — the null tag, wrong for undefined.
            //
            // Also observable via
            // `Object.getOwnPropertyDescriptor(...).value === undefined`
            // where the descriptor's Any-boxed undefined was compared
            // against a locally-bound `undefined`, blocking test262
            // `Object.create` `desc.value` cluster (8 case) among
            // others. Route the id-based flags first so the SSA-Type
            // shape of the concrete side no longer gates the tag.
            let (tag, value): (i64, Operand) = if concrete_is_undef {
                (5, Operand::ConstI64(0))
            } else if concrete_is_null {
                (0, Operand::ConstI64(0))
            } else {
                match concrete_ty {
                    Type::I64 | Type::I32 => (2, concrete),
                    Type::F64 => {
                        let bits = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::BitCastF64ToI64(concrete),
                            Type::I64,
                            None,
                        );
                        (3, Operand::Value(bits))
                    }
                    Type::Bool => {
                        let zext = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::ZExtBoolToI64(concrete),
                            Type::I64,
                            None,
                        );
                        (1, Operand::Value(zext))
                    }
                    t if t.is_refcounted() => (4, concrete),
                    _ => (0, Operand::ConstI64(0)),
                }
            };
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_strict_eq,
                    vec![any_box, Operand::ConstI64(tag), value],
                ),
                Type::Bool,
                None,
            );
            Operand::Value(r)
        };
        if matches!(op, AstBinOp::Neq) {
            let neg = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, result, Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(neg));
        }
        return Some(result);
    }
    let numeric = |t: Type| matches!(t, Type::I64 | Type::F64);
    // Pointer-shaped: strings, heap objects, the null literal,
    // and Any. Nullable<T> (check.rs notion) erases to the
    // underlying T at SSA — already covered. Comparing any of
    // these against null literal (Ptr) at runtime needs a real
    // pointer cmp, so they all share a family for fold purposes.
    // Chunk 738 — FnSig joins: a nullable fn-typed immutable slot
    // (`const f: (() => T) | null = null`) keeps FnSig repr (null
    // init isn't Expr::Closure, named-fn inits stay direct-
    // dispatch), and its value is a fn address or the null
    // sentinel — pointer-shaped either way. Excluding it folded
    // `f === null` to constant false.
    let pointerish = |t: Type| {
        use crate::ssa::Type::*;
        matches!(
            t,
            Ptr | Str
                | Substr
                | Obj(_)
                | Arr(_)
                | Closure(_)
                | FnSig(_)
                | Symbol
                | Promise
                | RegExp
                | Date
                | WeakRef
                | WeakMap
                | WeakSet
                | BigInt
                | Any
        )
    };
    let same_family =
        (numeric(a_ty) && numeric(b_ty)) || (pointerish(a_ty) && pointerish(b_ty)) || a_ty == b_ty;
    if !same_family {
        let answer = matches!(op, AstBinOp::Neq);
        return Some(Operand::ConstBool(answer));
    }
    None
}
