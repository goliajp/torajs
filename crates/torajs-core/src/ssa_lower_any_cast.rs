//! `Expr::As` cast lowering — the box/unbox bridge between the
//! typed tier and the NaN-box AnyValue representation. Split from
//! `ssa_lower.rs`'s `lower_expr_inner` (file-size known-debt:
//! ssa_lower.rs only shrinks).
//!
//! TS `as` is a compile-time assertion with no runtime conversion,
//! but tora's typed tier MATERIALIZES the annotated face: a
//! primitive entering an `any` slot boxes, and an Any value entering
//! a `number` slot unboxes (spec §7.1.4 ToNumber — exact round-trip
//! for number-tagged boxes, which is the only well-typed case).

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};

impl crate::ssa_lower::LowerCtx<'_> {
    pub(crate) fn lower_as_cast(&mut self, inner: ExprId, ty_ann: &str) -> Operand {
        let inner_op = self.lower_expr(inner);
        let inner_ty = self.operand_ty(&inner_op);
        if ty_ann == "any" {
            let is_primitive = matches!(inner_ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool);
            if is_primitive {
                return self.box_to_any_from_expr(inner, inner_op);
            }
            // RFC C4 — `undefined as any` / `null as any` arg-validation:
            // `Object.getOwnPropertyDescriptor(undefined as any, ...)` must
            // throw "undefined is not an object" (spec §10.1.6 ToObject).
            // SSA-lower collapses both `undefined` and `null` to
            // ConstPtrNull (Type::Ptr) at the value level; without this
            // path the call-site sees an un-boxed Ptr and the spec ANY_UNDEF
            // vs ANY_NULL discrimination collapses (both fall to
            // `box_to_any`'s `ConstPtrNull → ANY_NULL=0` arm, making
            // undefined misreport as null at runtime). Routing through
            // `box_to_any_from_expr` reads the `inner` expression's
            // `expr_types` (which still says Type::Undefined / Type::Null
            // before the As cast erased it to Any) and emits the
            // correct tag.
            if matches!(inner_ty, Type::Ptr) {
                return self.box_to_any_from_expr(inner, inner_op);
            }
            // S132 — refcounted typed reference (Obj / Arr / Str / Map /
            // Set / Date / RegExp / ...) `<v> as any`: TS spec is a
            // type-erase, but tora's materialize tier needs a real
            // box → NaN-box AnyValue cell-imm so downstream Any-arm
            // dispatchers (Object.values / Object.keys / Object.entries /
            // gOPD / inspect / etc.) see Type::Any and route through
            // the W-J walker. Without this branch the typed Obj fast
            // path in `Object.values` runs against a heterogeneous
            // struct (check.rs erased it to Type::Any but ssa-lower
            // still saw Type::Obj(sid)), silently reading every field
            // through `layout[0].1` and emitting wrong-type loads —
            // `Object.values({n:1, s:"x"} as any)` returns the str ptr
            // as a Number. Routing refcounted types through
            // `box_to_any_from_expr` reuses the existing primitive
            // path's tag-aware emit (ShortStr literal shortcut, etc.).
            if inner_ty.is_refcounted() && !matches!(inner_ty, Type::Any) {
                return self.box_to_any_from_expr(inner, inner_op);
            }
        }
        // Unbox direction (b1) — `<Any-valued> as number` must
        // materialize the numeric face. Pre-fix the boxed AnyValue
        // bits passed through raw and printed as garbage
        // (`m.get(k) as number` → NaN-box tag bits as integer).
        if inner_ty == Type::Any && ty_ann == "number" {
            return self.coerce_any_to_number(inner_op, Type::F64);
        }
        inner_op
    }
}
