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

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::num_width::SlotKey;
use crate::ssa::Type;
use crate::ssa_lower::parse_type;

// r290 file-size split — the K.3b un-annotated inference half.
mod gate;
mod infer;
mod write_through;
use gate::{mutable_promotes, slot_type_supported};
use infer::inferred_slot_ty;
use write_through::{binding_written_through, init_is_static_string_split};

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_toplevel_globals(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(PropKey, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, crate::ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> HashMap<String, Type> {
    let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
    // Rotation 204 — mirror of the checker's `dynobj_degraded`
    // consult in pass_2 (recomputed from this side's own Ast
    // snapshot, the standard no-drift contract).
    let dynobj_degraded = crate::dynobj_degrade::collect_dynobj_degraded_inits(ast);
    // Rotation 652 — same mirror for the cross-type widen set
    // (`check/construct.rs` holds the checker's copy).
    let cross_type_widened =
        crate::let_widen::collect_cross_type_widen_inits(ast, Some(expr_types));
    let mut globals: HashMap<String, Type> = HashMap::new();
    // Multi-flattened walk (rotation 230) — mirror of the checker's
    // pass_2 pre-pass, same no-drift contract.
    for stmt in crate::ast::toplevel_stmts_flat(ast) {
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
            // real slot so writes have somewhere to land. Chunk 809 —
            // an `any`-annotated binding never takes the inline fast
            // path: the checker registers it Any (annotation wins),
            // so named-fn reads expect a boxed slot, not a folded
            // scalar.
            if init_is_inline_literal && !*mutable && type_ann.as_deref() != Some("any") {
                continue;
            }
            // K.3b — slot type. With an annotation, parse it; without
            // one, run the gated shape inference. Both helpers answer
            // `None` for shapes that keep the K.1 main-local behavior.
            // An initializer whose own layout carries accessor slots
            // keeps that layout; the annotation only gated admission
            // in the checker (`check_accessor_objlit_slot`). Parsing
            // it here would hand the global a data field where the
            // value has a getter closure. The inference path is the
            // one the same literal takes unannotated, and it is right.
            //
            // The parse decides, not the annotation's presence: only
            // an `Obj` / `Arr` slot denotes a concrete LAYOUT that a
            // literal's accessor slots can conflict with. An `any`
            // annotation is a box whose dynamic member lane serves
            // accessors already — and setting it aside here loses the
            // global entirely, because the inference path answers
            // `None` for an object literal and the binding then has
            // no home at all (`const t: any = { get then() { … } }`
            // became `unknown ident t`, which the conformance gate
            // caught and no probe would have).
            let annotated = type_ann.as_ref().and_then(|ann| {
                annotated_slot_ty(
                    ann,
                    name,
                    *init,
                    ast,
                    expr_types,
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                    num_f64_slots,
                )
            });
            let accessor_layout = matches!(annotated, Some(Type::Obj(_) | Type::Arr(_)))
                && expr_types
                    .get(init)
                    .is_some_and(crate::check_accessor_objlit_slot::layout_carries_accessor);
            let slot_ty = match annotated.filter(|_| !accessor_layout) {
                Some(t) => Some(t),
                None if type_ann.is_some() && !accessor_layout => None,
                None => inferred_slot_ty(
                    name,
                    *init,
                    ast,
                    &binding_refs,
                    &dynobj_degraded,
                    &cross_type_widened,
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
            // RFC 20260912-global-var-cell — a script's top-level `var`
            // is the global object's property over the same storage,
            // and the cell that makes it so needs a stable address.
            // Only the `any` slot — what every hoisted `var` is — takes
            // the global home unconditionally. A typed escape-hatch
            // `var` (`var log: string[] = []`) keeps the gates below:
            // promoting one with no named-fn reader moves it onto the
            // global lane whose member-call lane does not carry it
            // (rotation 514's finding; the first gate of 636 put
            // `log.push` on "value is not a function"), so those bind
            // only when the checker promotes them anyway.
            if ast.script_global_var_names.contains_key(name) && ty == Type::Any {
                globals.insert(name.clone(), ty);
                continue;
            }
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
            // named fn could possibly read it. Refcounted slots take
            // the gate further down (`mutable_promotes`), where the
            // main-local home's scope-drop walk owns their cleanup.
            // r290 (closure-capture sweep cluster) — `named_fn_refs`
            // also counts captures of closures minted INSIDE named fn
            // bodies (see `idents_in_expr`'s Closure arm): those
            // resolve through the globals table (a named fn has no
            // env machinery), so localizing the binding would leave
            // the lifted body's name with no home ("closure capture
            // not in scope"). Top-level-positioned closures keep the
            // main-local capture-box machinery and do NOT widen this
            // gate.
            //
            // Rotation 514 — the family the refcounted gate admitted
            // localizes on those same terms, and for a sharper reason
            // than the primitives': it was admitted to give a named-fn
            // body something to read, while on the MAIN path such a
            // binding already works, method calls included. Promoting
            // one with no named-fn reader only moves it off that path
            // onto the global one, whose member-call lane does not
            // carry it — `const m: Map<…> = new Map(); m.set(…)` went
            // from working to "unsupported member call shape". The
            // historical Str / Arr / Obj / Closure / Symbol pass here
            // and meet the named-fn gate below instead.
            let visibility_only = ty.is_refcounted()
                && !matches!(
                    ty,
                    Type::Str
                        | Type::Arr(_)
                        | Type::Obj(_)
                        | Type::Closure(_)
                        | Type::Symbol
                        | Type::Any
                );
            if (matches!(ty, Type::I64 | Type::F64 | Type::Bool | Type::I32) || visibility_only)
                && !binding_refs.named_fn_refs.contains(name)
            {
                continue;
            }
            if !slot_type_supported(&ty, name, &binding_refs, ast) {
                continue;
            }
            // A `const` container localizes on the same terms as a
            // `let` one: with no named-fn reader the main-local home
            // already owns its drop, and the global home made every
            // hot-loop `xs.push` / `xs[i]` a GlobalRef load the push-
            // loop lane and the element-interval analysis do not see
            // through (`const xs: number[] = []` ran generic-id-1m at
            // 3.8x the `let` spelling). Str stays global: a literal
            // init would fall to the per-read K.1 re-alloc above.
            let gated = *mutable || !matches!(ty, Type::Str);
            if gated && ty.is_refcounted() && !mutable_promotes(&ty, name, &binding_refs) {
                continue;
            }
            globals.insert(name.clone(), ty);
        }
    }
    globals
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
/// A variadic ann parses to Closure already (the boxed dual entry
/// lives in the env cell) and takes the global home on the same
/// terms — its calls route off `Ast::variadic_value_bindings`. It
/// keeps the lifted-arrow condition for the same reason the re-repr
/// above does: only that init shape mints the env cell K.4 needs.
#[allow(clippy::too_many_arguments)]
fn annotated_slot_ty(
    ann: &str,
    name: &str,
    init: ExprId,
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(PropKey, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
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
    // Rotation 357 — the arrow probe peels `as` casts (`(<arrow>) as
    // any` is the probe-abi1 shape): the cast is a value-layer
    // pass-through, so the init still mints the env cell a FnSig
    // slot would dispatch as a raw code address (RFC
    // 20260810-indirect-argc-abi L3b ③).
    let mut peeled = init;
    while let Expr::As { expr, .. } = ast.get_expr(peeled) {
        peeled = *expr;
    }
    let init_is_lifted_arrow = matches!(ast.get_expr(peeled), Expr::Closure { .. });
    // Any other Any-typed init (an any-bound ident, an any member
    // read) carries the cell shape too, but K.4's global-init store
    // lane has no Any→Closure conversion — keep the binding
    // main-local instead, where the fn-local let lane converts
    // (`initial_let_ty` mirror). A named-fn read of such a binding
    // stays the recorded main-local unknown-ident limitation.
    if matches!(parsed, Type::FnSig(_))
        && !init_is_lifted_arrow
        && matches!(expr_types.get(&init), Some(crate::check::Type::Any))
    {
        return None;
    }
    let parsed = match parsed {
        Type::FnSig(sig) if !ann.contains("__rest(") && init_is_lifted_arrow => Type::Closure(sig),
        Type::Closure(sig) if ann.contains("__rest(") && init_is_lifted_arrow => Type::Closure(sig),
        // An ALIAS of a lifted-arrow binding carries the same env
        // cell the arrow itself mints, so it takes the same slot —
        // `const gb: (...xs: number[]) => number = ga` was "unknown
        // function `gb`" from a named-fn body because the binding
        // never promoted. Resolved through the one-hop spelling that
        // types an UNannotated alias, so the annotated and the bare
        // form of the same line agree.
        Type::Closure(sig)
            if ann.contains("__rest(")
                && crate::ast_refs::closure_alias_fn_canon(ast, peeled).is_some() =>
        {
            Type::Closure(sig)
        }
        Type::Closure(_) if ann.contains("__rest(") => return None,
        // `const a: string[] = s.split(" ")` — the annotation spells
        // Arr<Str>, but the init the K.4 lane will store is what
        // `lower_split` answers for a statically-string separator:
        // an array of Substr VIEWS (the split block's inline cells).
        // Typing the slot by the annotation made every Arr<Str>-
        // dispatched reader (join / JSON.stringify / for-of / .at /
        // HOF callbacks) decode a 32-byte view as an owned Str and
        // print its parent pointer as text. Same move as the
        // FnSig→Closure arm above: the slot takes the representation
        // the init actually produces. `let` never reached here (it is
        // not promoted) and was always right.
        Type::Arr(aid)
            if arr_layouts[aid.0 as usize] == Type::Str
                && init_is_static_string_split(peeled, ast, expr_types) =>
        {
            // Written through (push / sort / a[i] = v / …): an
            // Arr<Substr> slot cannot take an owned Str, and keeping
            // the annotation's Arr<Str> is the original bug (views in
            // a Str-typed slot). Neither layout is right for a global
            // — so it is not promoted, and stays the main-local the
            // `let` spelling already is.
            if binding_written_through(ast, name) {
                return None;
            }
            Type::Arr(crate::ssa_lower::intern_arr_layout(
                arr_layouts,
                Type::Substr,
            ))
        }
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
