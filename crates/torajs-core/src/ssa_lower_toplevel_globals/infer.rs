//! K.3b un-annotated slot-type inference — the `inferred_slot_ty`
//! half of the toplevel-globals collection, split out of the parent
//! when the r290 closure-capture gates pushed it past the 500-line
//! cap. Child module: reaches the parent-crate helpers through
//! ordinary paths; the parent calls back through `pub(super)`.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::ast_refs::GlobalSlotShape;
use crate::num_width::SlotKey;
use crate::ssa::Type;
use crate::ssa_lower::parse_type;

fn slot_shape_to_type(shape: GlobalSlotShape) -> Type {
    match shape {
        GlobalSlotShape::I64 => Type::I64,
        GlobalSlotShape::F64 => Type::F64,
        GlobalSlotShape::Str => Type::Str,
        GlobalSlotShape::Bool => Type::Bool,
        GlobalSlotShape::Symbol => Type::Symbol,
    }
}

/// K.3b — slot type for an UN-ANNOTATED top-level binding. Promotes
/// only behind the ast_refs gate — a named-fn body must reference the
/// binding (named fns have no capture machinery), and a MUTABLE
/// closure-captured binding stays main-local (its env-copy capture
/// home would disagree with the slot; immutable captures resolve to
/// the global through the capture filter instead — chunk 737).
/// `None` keeps the binding main-local (K.1 behavior).
///
/// RFC 20260709-closure-global chunk 2 — a lifted-arrow init promotes
/// under the sig synthesized from the lifted FnDecl's
/// (preinfer-backfilled) anns; `annotated_slot_ty` then rides the
/// exact annotated lane (FnSig → Closure re-repr, variadic guard).
/// Mutable bindings fall through to the caller's K.6 refcount gate
/// and stay main-local until the RFC's assign-lane chunk — mirroring
/// the checker's pass_2 registration gate.
///
/// Other init shapes go through the shared shape inference; an
/// inferred `number` widens to F64 when W1's module-wide width table
/// says any reaching value is f64-possible (storing f64 bits in an
/// i64 slot reinterprets the payload as a garbage integer on every
/// read).
#[allow(clippy::too_many_arguments)]
pub(super) fn inferred_slot_ty(
    name: &str,
    init: ExprId,
    ast: &Ast,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
    dynobj_degraded: &std::collections::HashSet<ExprId>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, crate::ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> Option<Type> {
    // Chunk 737 — an IMMUTABLE closure-captured binding promotes:
    // once the slot exists, the closure-construction capture filter
    // (`eff_captures`, K.3/K.4/K.6 arm) resolves the name to the
    // global and the lifted body reads it through GlobalRef exactly
    // like a named-fn body — no env copy, so no second home to
    // disagree with. Chunk 740 — MUTABLE captured bindings promote
    // the same way: the lifted body's writes take the Assign-Ident
    // global lane, so reads and writes share the one global home
    // (the old env-copy snapshot disagreed with ES shared-binding
    // semantics — `inc(); show()` read a stale main-local). r290 —
    // `named_fn_refs` also counts captures of closures minted inside
    // named fn bodies (`idents_in_expr`'s Closure arm), so a nested
    // fn-expr's capture of a top-level binding admits here without a
    // direct named-fn read.
    if !binding_refs.named_fn_refs.contains(name) {
        return None;
    }
    // Rotation 204 — a dynobj-degraded ObjectLit init promotes as
    // Any, the exact type the checker's pass_2 registered for it
    // (degrade ≡ `: any` annotation). The Any slot rides the whole
    // chunk-809 Any-global machinery: supported/mutable gates admit
    // it, the init lane boxes a fresh dynobj, the exit hook's Any
    // arm settles the box.
    if dynobj_degraded.contains(&init) {
        return Some(Type::Any);
    }
    if let Expr::Closure { fn_name, .. } = ast.get_expr(init) {
        let canon = crate::ast_refs::lifted_closure_fn_canon(ast, fn_name)?;
        let parsed = parse_type(
            Some(&canon),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        // Variadic sigs (`__rest(` spellings parse to Closure
        // directly) keep the main-local home — the boxed-dual call
        // routing rides the fn-local `variadic_locals` table (RFC O2).
        let Type::FnSig(sig) = parsed else {
            return None;
        };
        // F5 shape, NOT the F1 canon class: with no annotation
        // written, the binding never joined the spelling's nominal
        // class — its widths live on the slot key's `__ret` / `__p{i}`
        // projections, glued to the lifted fn's Ret / Param keys by
        // the let site's `fn_value_flow`. Querying the canon class
        // here would answer stale parse widths and the env-first
        // CallIndirect would read a floated callee's d0 ret off x0
        // (the untouched env pointer). Joining the class instead is
        // wrong the other way: it glues unrelated same-spelling
        // residents' widths together.
        return Some(crate::ssa_lower_container_width::widen_fn_sig_by_key(
            Type::Closure(sig),
            &SlotKey::Global(name.to_string()),
            num_f64_slots,
            fn_sigs,
        ));
    }
    // RFC 20260725 follow-up — an un-annotated all-literal ObjectLit
    // init promotes under its synthesized `__inlobj(...)` spelling
    // (the exact string the checker's pass_2 registered, resolved
    // through the same parse pipeline — layout, field widths and the
    // interned sid all unify with an equivalent written annotation).
    if let Some(ann) = crate::ast_refs::objlit_literal_inlobj_ann(ast, init) {
        let parsed = parse_type(
            Some(&ann),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        // Same widen the annotated lane runs: an I64 field whose
        // alias-class point is f64-possible (`w.a = 2.5` in a named
        // fn) re-interns as F64 instead of tripping the
        // assign-member width reject.
        return Some(crate::ssa_lower_container_width::widen_container_ty(
            parsed,
            Some(&ann),
            &SlotKey::Global(name.to_string()),
            num_f64_slots,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        ));
    }
    // Cluster-`values` follow-up (rotation 253) — an un-annotated
    // all-literal Array init promotes under its synthesized `T[]`
    // spelling (the exact string the checker's pass_2 registered,
    // resolved through the same parse pipeline — the `__inlobj`
    // precedent above). Same widen the annotated lane runs.
    if let Some(ann) = crate::ast_refs_arrlit::arrlit_literal_elem_ann(ast, init) {
        let parsed = parse_type(
            Some(&ann),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        return Some(crate::ssa_lower_container_width::widen_container_ty(
            parsed,
            Some(&ann),
            &SlotKey::Global(name.to_string()),
            num_f64_slots,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        ));
    }
    let Some(shape) = crate::ast_refs::infer_toplevel_slot_shape(ast, init) else {
        // S2.35 — a call-result init the shape inference can't type
        // promotes as an Any slot via the shared verdict the
        // checker's pass_2 registered from (`ast_refs_any_promote`,
        // same fallback position — the no-drift contract). The Any
        // slot rides the chunk-809 machinery: the init lane boxes
        // the concrete value, reads dispatch any-lane. Shape-typed
        // calls above keep their exact slot.
        if crate::ast_refs_any_promote::any_promote_init(ast, init) {
            return Some(Type::Any);
        }
        return None;
    };
    let parsed = slot_shape_to_type(shape);
    Some(
        if parsed == Type::I64 && num_f64_slots.slot_is_f64(&SlotKey::Global(name.to_string())) {
            Type::F64
        } else {
            parsed
        },
    )
}
