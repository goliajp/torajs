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
        // An object literal cast to `any` promotes to the dynobj
        // lane (twin of the empty-[] → Arr<Any> promote and of the
        // direct-ObjectLit call-arg route in
        // `ssa_lower_call_terminal`): the struct lane would pass an
        // anon struct through the cast, and every downstream
        // consumer of the `any` face — descriptor walks
        // (`Object.defineProperties({} as any, props)` eval-dropped
        // on the tag-gate miss), any-member dispatch (a this-using
        // method's nominal `__this` read struct offsets off the
        // boxed face = SIGSEGV) — needs the dynobj shape. Was
        // empty-literal-only (rotation 125); RFC
        // 20260717-objlit-anylane-recv knife 2 widened it to every
        // literal, paired with the AST-side anylane predicate's (c)
        // leg so recv members get the `__this: any` shape.
        if ty_ann == "any"
            && let crate::ast::Expr::ObjectLit { .. } = self.ast.get_expr(inner)
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
        // Unbox direction (b1) — `<Any-valued> as T` must materialize
        // the annotated primitive face. Pre-fix only `number` had an
        // arm; `as string` / `as boolean` / `as bigint` passed the
        // NaN-box bits through raw, which downstream deref'd as a
        // heap pointer (silent no-output or SIGSEGV depending on
        // layout). Same helpers as the call-lane arg_conv contract;
        // string / bigint mint an owned value the consuming lane
        // releases like any other owned temp.
        if inner_ty == Type::Any {
            match ty_ann {
                "number" => return self.coerce_any_to_number(inner_op, Type::F64),
                "string" => return self.coerce_to_str(inner_op, Type::Any),
                "boolean" => return self.coerce_to_bool(inner_op),
                "bigint" => return self.coerce_any_to_bigint(inner_op),
                _ => {}
            }
        }
        inner_op
    }
}
