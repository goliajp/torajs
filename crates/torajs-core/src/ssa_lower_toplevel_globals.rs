//! Pass 1.5 (K.3) — registration of top-level data globals, plus the
//! localize gate that keeps main-only bindings out of the global
//! space.
//!
//! A top-level `let X: T = init` whose type annotation parses to a
//! primitive Copy type (I64 / F64 / Bool / I32) and whose initializer
//! is NOT a literal becomes a real LLVM global slot — readable and
//! writable from named-fn bodies via `GlobalRef + Load` / `+ Store`.
//!
//! Skipped (still scope to implicit main as a local):
//!   - literal-init forms (`const X = 42`) — the K.1 inline-literal
//!     fallback path is faster and doesn't need a slot.
//!   - missing type annotation without a named-fn reader — K.3
//!     doesn't run inference here; `let Y = computeValue()` without
//!     `: T` keeps the K.1 behavior of being a main-fn local
//!     (named-fn read errors with "unknown ident"). The K.3b
//!     `ast_refs` gate promotes the subset a named fn actually reads.
//!   - annotated primitive bindings that NO named-fn body references
//!     (the localize gate) — the slot exists solely for named-fn
//!     visibility, so a main-only binding keeps the local home and
//!     the slot-promotion family dissolves it into registers.
//!   - mutable refcount-typed annotations (Arr / Obj) — hidden
//!     mutation through method calls (`xs.push(v)`) would need
//!     writeback to the slot (K.6, not yet landed).

use std::collections::HashMap;

use crate::ast::{Ast, Expr, Stmt};
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
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_toplevel_globals(
    ast: &Ast,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> HashMap<String, Type> {
    let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
    let mut globals: HashMap<String, Type> = HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
            mutable,
            is_var: false,
        } = stmt
        {
            // Number / Bool literal init stays on the K.1 fast path —
            // those are Copy types so inlining the constant at every
            // read is free. String literal init must go through the
            // globals path: K.1's fallback emits a fresh
            // `__torajs_str_alloc` per read site, which leaks one
            // alloc per read (uncovered by `m-oo-04-static`'s leak
            // audit — `Counter.label !== "ctr"` was paying a fresh
            // alloc on the LHS at every comparison).
            let init_is_inline_literal =
                matches!(ast.get_expr(*init), Expr::Number(_) | Expr::Bool(_));
            // V3-18 m1.h.26 — only the IMMUTABLE inline-literal case
            // can be inlined at every read. Mutable globals (e.g.
            // static class fields like `Counter.value = 0`) need a
            // real slot so writes have somewhere to land.
            if init_is_inline_literal && !*mutable {
                continue;
            }
            // K.3b — slot type. With an annotation, parse it; "number"
            // parses to the I64 default and W1's module-wide width
            // inference widens to F64 when any reaching value
            // (initializer OR a later assignment, including from a
            // named-fn body) is f64-possible — storing f64 bits in an
            // i64 slot reinterprets the payload as a garbage integer
            // on every read. Without an annotation, promote only
            // behind the ast_refs gate — a named-fn body must
            // reference the binding (named fns have no capture
            // machinery) and no closure may capture it (captures copy
            // through __env from the main-fn local; a slot would
            // split the binding into two disagreeing homes). Shapes
            // the shared inference can't resolve keep the K.1
            // main-local behavior.
            let widened = |parsed: Type| {
                if parsed == Type::I64 && num_f64_slots.slot_is_f64(&SlotKey::Global(name.clone()))
                {
                    Type::F64
                } else {
                    parsed
                }
            };
            let ty = match type_ann {
                Some(ann) => {
                    let parsed = parse_type(
                        Some(ann),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                    );
                    // W4 — container elem widths from the alias-class
                    // table (same consult as the fn-local let site).
                    let parsed = crate::ssa_lower_container_width::widen_arr_elem(
                        parsed,
                        Some(ann),
                        &SlotKey::Global(name.clone()),
                        num_f64_slots,
                        arr_layouts,
                    );
                    if ann == "number" {
                        widened(parsed)
                    } else {
                        parsed
                    }
                }
                None => {
                    if !binding_refs.named_fn_refs.contains(name)
                        || binding_refs.closure_captured.contains(name)
                    {
                        continue;
                    }
                    match crate::ast_refs::infer_toplevel_slot_shape(ast, *init) {
                        Some(shape) => widened(slot_shape_to_type(shape)),
                        None => continue,
                    }
                }
            };
            // Localize — the data-global slot exists solely so
            // capture-less named-fn bodies can see a top-level binding
            // (the ast_refs contract above). An annotated primitive-
            // Copy slot that NO named-fn body references has no reader
            // outside the implicit main, so the global home only pins
            // every hot-loop access to a GlobalRef + Load/Store that
            // the slot-promotion family (slot_forward / mem2reg /
            // phi_promote) cannot touch. Keep it a main-fn local
            // instead — same semantics (top-level script order), and
            // the alloca dissolves into registers like any other
            // local. `named_fn_refs` over-approximates (fn-local
            // shadows count), so a binding only localizes when no
            // named fn could possibly read it. Refcounted slots
            // (Str / Arr / Obj) keep the global path: their promotion
            // is tied to the exit-time drop hook, not to named-fn
            // visibility alone.
            if matches!(ty, Type::I64 | Type::F64 | Type::Bool | Type::I32)
                && !binding_refs.named_fn_refs.contains(name)
            {
                continue;
            }
            // K.3 — primitive Copy types (no lifetime concerns).
            // K.4 — refcount Str (drop on program exit).
            // K.6 — refcount Arr / Obj (same drop machinery as Str —
            //       `emit_drop_value` dispatches by type, walking
            //       refcounted array elements / object fields).
            // Closure / FnSig still deferred: Closure needs the
            // matching `__env_drop_<closure>` to wire through the
            // global-drop path, and FnSig globals haven't surfaced
            // a real use case yet.
            // Type::Any is intentionally NOT in the supported set —
            // promoting it triggers a load/store-shape mismatch when
            // the slot holds an any-box wrapper while reads expect
            // dynobj content (Member access goes through dynobj_get
            // on val@+16). P4.5's `new.target` instead reaches the
            // class via the runtime classes-by-tag side table:
            // factory bodies call `__torajs_my_class_ref("<C>")`
            // which ssa_lower intercepts → class_get(<tag>).
            let supported = matches!(
                ty,
                Type::I64
                    | Type::F64
                    | Type::Bool
                    | Type::I32
                    | Type::Str
                    | Type::Arr(_)
                    | Type::Obj(_)
            );
            if !supported {
                continue;
            }
            // K.6 — mutable refcount globals are not yet supported.
            // The shipped Assign-Ident reject covers `X = newValue`,
            // but hidden mutation through method calls (`xs.push(v)`,
            // `xs.sort()`, `obj.field = v` on a global) bypasses
            // that gate and would need writeback to the global slot
            // for any push that reallocates. Until that path lands,
            // mutable refcount globals stay scoped to the implicit
            // main as before. Mutable primitive Copy globals stay
            // promoted (K.3 / globals-001 depends on it).
            if *mutable && ty.is_refcounted() {
                continue;
            }
            globals.insert(name.clone(), ty);
        }
    }
    globals
}
