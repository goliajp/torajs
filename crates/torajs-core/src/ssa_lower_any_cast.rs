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
use crate::ssa::{InstKind, Operand, Type};

impl crate::ssa_lower::LowerCtx<'_> {
    pub(crate) fn lower_as_cast(&mut self, eid: ExprId, inner: ExprId, ty_ann: &str) -> Operand {
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
        // A literal cast to a named struct type is stored in that
        // type's layout, the same as under an annotation: the cast
        // value is read at the declared offsets.
        if let crate::ast::Expr::ObjectLit { .. } = self.ast.get_expr(inner)
            && let Some(Type::Obj(sid)) = self.aliases.get(ty_ann)
        {
            self.let_declared_obj_layout = Some(*sid);
        }
        let inner_op = self.lower_expr(inner);
        self.let_declared_obj_layout = None;
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
            // A heap value otherwise passes through this cast wearing
            // its static SSA type, which is right until that type is
            // the only thing that knows the slot's `undefined` is an
            // in-band address. After the cast nothing downstream can
            // ask the question any more — the type says `any` and the
            // source is an `As` — so `"x" + (r as any)` walked the
            // immortal cell and `(r as any) * 2` called it. Re-encode
            // here, where both halves of the question are still in
            // hand; `box_to_any_from_expr` spells it branchlessly
            // (tag = 4 + (val == sentinel)).
            if crate::ssa_lower_nullable_guard::holds_undef_cell(self, inner, inner_ty) {
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
            use crate::ssa_lower_checked_cast::{Admits, CastScope, CastSlot};
            let target = match ty_ann {
                "number" => Type::F64,
                "string" => Type::Str,
                "boolean" => Type::Bool,
                _ => Type::Void,
            };
            // a string or bigint face is a fresh value the binding
            // that takes it owns, not a view of the element it read
            if matches!(ty_ann, "string" | "bigint") {
                self.owned_member_reads.insert(eid);
            }
            if let Some(v) = self.checked_cast(
                inner_op,
                target,
                CastSlot::Cast,
                CastScope::Annotated,
                Admits::Exact,
            ) {
                return v;
            }
            match ty_ann {
                "number" => return self.coerce_any_to_number(inner_op, Type::F64),
                // a cast is no ToString: `null` / `undefined` keep the
                // Str slot's own spelling of them, as at a `string`
                // let annotation (`ssa_lower_stmt_let_decl_convert`)
                "string" => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_to_str_slot, vec![inner_op]),
                        Type::Str,
                        None,
                    );
                    return Operand::Value(v);
                }
                "boolean" => return self.coerce_to_bool(inner_op),
                "bigint" => return self.coerce_any_to_bigint(inner_op),
                _ => {}
            }
            // The same materialization for the annotations that name
            // ONE heap layout — `Map<K, V>`, `Set<T>`, `Date`,
            // `RegExp`, the weak collections, the iterator cells, a
            // `Promise<T>`. Without it a cast out of `any` stayed a
            // NaN-box while the checker had already answered the
            // annotated type, so every typed lane that dispatches on
            // the operand — a Map method call, a `const m: Map<…> =
            // o.m as Map<…>` binding — either declined the receiver
            // it was written for or would have read a header off a
            // boxed word.
            //
            // The decode BORROWS (`any_cell_ptr` materializes
            // nothing), so ownership still follows the inner
            // expression, which is what the `As`-peeling done at the
            // return / throw / field-store / let-share sites reads.
            // Dropping the cell through its `Map` face is the same
            // decrement dropping the box would have made.
            if let Some(face) = self
                .expr_types
                .get(&eid)
                .and_then(crate::ssa_lower_recv_face::monomorphic_face)
            {
                let raw = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_cell_ptr, vec![inner_op]),
                    Type::I64,
                    None,
                );
                let cell = self.f.append_inst(
                    self.cur_block,
                    InstKind::IntToPtr(Operand::Value(raw)),
                    face,
                    None,
                );
                return Operand::Value(cell);
            }
        }
        inner_op
    }
}
