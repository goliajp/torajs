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
        // An EMPTY object literal cast to `any` promotes to the
        // dynobj lane (rotation 125 L3b; twin of the empty-[] →
        // Arr<Any> promote and of the direct-ObjectLit call-arg
        // route in `ssa_lower_call_terminal`): the struct lane would
        // pass a zero-field anon struct through the cast, and
        // runtime descriptor walks (`Object.defineProperties({} as
        // any, props)`) silently eval-drop on the tag-gate miss.
        if ty_ann == "any"
            && let crate::ast::Expr::ObjectLit { fields } = self.ast.get_expr(inner)
            && fields.is_empty()
        {
            let dynobj = self.lower_dynobj_init(inner);
            // ANY_HEAP encode so downstream gates see a true Any face
            // (the raw Ptr face missed `defineProperties`' obj_ty gate
            // and kept the eval-drop leg).
            return self.box_to_any(dynobj);
        }
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
