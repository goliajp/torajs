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

use crate::ast::{Ast, Expr, ExprId, Stmt};
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
    inst_memo: &mut HashMap<String, crate::ssa::StructId>,
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
            // K.3b — slot type. With an annotation, parse it; without
            // one, run the gated shape inference. Both helpers answer
            // `None` for shapes that keep the K.1 main-local behavior.
            let slot_ty = match type_ann {
                Some(ann) => annotated_slot_ty(
                    ann,
                    name,
                    *init,
                    ast,
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                    num_f64_slots,
                ),
                None => inferred_slot_ty(
                    name,
                    *init,
                    ast,
                    &binding_refs,
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                    num_f64_slots,
                ),
            };
            let Some(ty) = slot_ty else {
                continue;
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
            // RFC 20260709-closure-global chunk 1 — Closure joins:
            // the drop machinery dispatches by type (env drop_fn
            // @+16, chunk 530), init is a fresh lifted env (K.4
            // fresh-heap-init holds), reads ride the global-closure
            // CallIndirect lane. Mutable closure globals stay
            // deferred to the RFC's assign-lane chunk.
            let supported = matches!(
                ty,
                Type::I64
                    | Type::F64
                    | Type::Bool
                    | Type::I32
                    | Type::Str
                    | Type::Arr(_)
                    | Type::Obj(_)
                    | Type::Closure(_)
            );
            if !supported {
                continue;
            }
            // K.6 — mutable Arr / Obj globals are not yet supported.
            // The shipped Assign-Ident reject covers `X = newValue`,
            // but hidden mutation through method calls (`xs.push(v)`,
            // `xs.sort()`, `obj.field = v` on a global) bypasses
            // that gate and would need writeback to the global slot
            // for any push that reallocates. Until that path lands,
            // mutable Arr / Obj globals stay scoped to the implicit
            // main as before. Mutable primitive Copy globals stay
            // promoted (K.3 / globals-001 depends on it). Mutable Str
            // globals promote ONLY behind the named-fn-refs gate
            // (chunk 558): strings have no in-place mutation methods,
            // so the writeback concern doesn't exist and the
            // Assign-Ident path handles drop-old/store-new — but the
            // slot's sole purpose is named-fn visibility, and
            // unconditional promotion drags await-init / borrow-
            // shaped-init bindings (`let s: string = await p`) into
            // the K.4 fresh-heap-init requirement they don't meet.
            // No named-fn reader/writer → keep the main-local home
            // (its scope-drop walk already owns cleanup).
            // Chunk 730 (RFC 20260709-closure-global) — mutable
            // Closure globals promote behind the same gate: a
            // closure's env is opaque to user code (no in-place
            // mutation surface), assignment is the only mutation
            // face and the Assign-Ident lane owns
            // drop-old/store-new. Chunk 740 — closure-captured
            // bindings promote too: the capture filter resolves the
            // name to the global for reads AND the lifted body's
            // writes take the same Assign-Ident global lane, so the
            // slot IS the single home (the old env-copy snapshot
            // disagreed with ES shared-binding semantics).
            let mutable_promote = (ty == Type::Str || matches!(ty, Type::Closure(_)))
                && binding_refs.named_fn_refs.contains(name);
            if *mutable && ty.is_refcounted() && !mutable_promote {
                continue;
            }
            globals.insert(name.clone(), ty);
        }
    }
    globals
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
fn inferred_slot_ty(
    name: &str,
    init: ExprId,
    ast: &Ast,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
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
    // semantics — `inc(); show()` read a stale main-local).
    if !binding_refs.named_fn_refs.contains(name) {
        return None;
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
    let shape = crate::ast_refs::infer_toplevel_slot_shape(ast, init)?;
    let parsed = slot_shape_to_type(shape);
    Some(
        if parsed == Type::I64 && num_f64_slots.slot_is_f64(&SlotKey::Global(name.to_string())) {
            Type::F64
        } else {
            parsed
        },
    )
}

/// K.3b — slot type for an ANNOTATED top-level binding. `None` keeps
/// the binding main-local.
///
/// W4 — container elem widths come from the alias-class table (same
/// consult as the fn-local let site).
///
/// RFC 20260709-closure-global chunk 1 — a fn-type annotation parses
/// to FnSig (direct-dispatch repr), but the global slot holds Closure
/// values (lifted arrows mint env cells), so re-repr over the same
/// interned sig (struct-field `__cls` precedent). The re-repr only
/// fires when the init IS a lifted arrow (`Expr::Closure` — every
/// arrow lifts to it regardless of capture count since
/// P3.closure-in-struct-field): only that shape mints the fresh env
/// cell K.4 requires. A named-fn reference init
/// (`const f: (x)=>y = take`) lowers to a borrow-shaped FnAddr and
/// must keep the main-local home (the fn_addr_let lane handles it);
/// wrapping it in a forwarder env is the RFC's chunk-4 station.
/// Variadic anns keep the main-local home too: the boxed-dual call
/// routing rides the fn-local `variadic_locals` table (RFC O2).
#[allow(clippy::too_many_arguments)]
fn annotated_slot_ty(
    ann: &str,
    name: &str,
    init: ExprId,
    ast: &Ast,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, crate::ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> Option<Type> {
    let parsed = parse_type(
        Some(ann),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    );
    let parsed = crate::ssa_lower_container_width::widen_container_ty(
        parsed,
        Some(ann),
        &SlotKey::Global(name.to_string()),
        num_f64_slots,
        arr_layouts,
        struct_layouts,
        fn_sigs,
    );
    let init_is_lifted_arrow = matches!(ast.get_expr(init), Expr::Closure { .. });
    let parsed = match parsed {
        Type::FnSig(sig) if !ann.contains("__rest(") && init_is_lifted_arrow => Type::Closure(sig),
        Type::Closure(_) if ann.contains("__rest(") => return None,
        t => t,
    };
    Some(
        if ann == "number"
            && parsed == Type::I64
            && num_f64_slots.slot_is_f64(&SlotKey::Global(name.to_string()))
        {
            Type::F64
        } else {
            parsed
        },
    )
}
