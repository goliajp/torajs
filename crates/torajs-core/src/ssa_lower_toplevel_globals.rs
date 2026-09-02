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
mod infer;
mod write_through;
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
            let slot_ty = match type_ann {
                Some(ann) => annotated_slot_ty(
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
                ),
                None => inferred_slot_ty(
                    name,
                    *init,
                    ast,
                    &binding_refs,
                    &dynobj_degraded,
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
            // historical Str / Arr / Obj / Closure / Symbol keep
            // promoting unconditionally per the paragraph above.
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
            if *mutable && ty.is_refcounted() && !mutable_promotes(&ty, name, &binding_refs) {
                continue;
            }
            globals.insert(name.clone(), ty);
        }
    }
    globals
}

/// Whether `ty` is a slot type the data-global machinery carries at
/// all (the `supported` gate, extracted verbatim rotation 297).
///
/// K.3 — primitive Copy types (no lifetime concerns).
/// K.4 — refcount Str (drop on program exit).
/// K.6 — refcount Arr / Obj (same drop machinery as Str —
///       `emit_drop_value` dispatches by type, walking refcounted
///       array elements / object fields).
/// Rotation 514 — the list was an enumerated subset of exactly that
/// machinery's domain, so every OTHER refcounted slot type the
/// CHECKER registers (Map / Set / Date / RegExp / Promise / BigInt /
/// the Weak family / Substr / the iterators) was registered on one
/// side and refused on the other: `let d: Date = new Date()` plus any
/// `function f() { … d … }` typechecked and then died in lowering
/// with "unknown ident `d`". The two sides disagreeing is worse than
/// either answer alone, so the gate now asks the question the lines
/// above already answer — is this a slot `emit_drop_value` drops?
/// FnSig stays deferred and is not refcounted (a bare code address);
/// FnSig globals haven't surfaced a real use case yet.
/// RFC 20260709-closure-global chunk 1 — Closure joins: the drop
/// machinery dispatches by type (env drop_fn @+16, chunk 530), init
/// is a fresh lifted env (K.4 fresh-heap-init holds), reads ride the
/// global-closure CallIndirect lane. Chunk 809 — Type::Any joins for
/// USER `any`-annotated bindings a named fn reads: the slot holds a
/// NaN-box AnyValue (the same 8-byte repr every Arr<Any> slot
/// carries), init boxes through `box_to_any_from_expr`, the assign
/// lane drops-old / boxes-new, and the exit hook's `emit_drop_value`
/// Any arm settles the box. The historical dynobj-shape mismatch
/// concern predates the unified NaN-box repr. Synthetic class
/// plumbing (`__class_*` / `__proto_*`, `: any`-annotated by
/// construction) stays main-local — the class machinery owns its own
/// access lanes. Cluster #4 follow-up (rotation 235) — Symbol joins:
/// fresh-mint init (§20.4.1), no in-place mutation surface (same
/// profile as Str), drop dispatch already carries a Symbol arm
/// (`symbol_drop`).
fn slot_type_supported(
    ty: &Type,
    name: &str,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
    ast: &Ast,
) -> bool {
    matches!(ty, Type::I64 | Type::F64 | Type::Bool | Type::I32)
        || (ty.is_refcounted() && *ty != Type::Any)
        || (*ty == Type::Any
        && binding_refs.named_fn_refs.contains(name)
        // Desugar-minted sentinels stay locals — EXCEPT the
        // computed-field key globals (RFC 20260802 刀 3 后半):
        // `__ccmk_<C>_<n>` holds the class-definition-time evaluated
        // key that the `__new_<C>` factory's ctor prefix reads per
        // construction — and the static-field slots (rotation 346):
        // `__sf_<C>__<n>` is the class's static storage, exactly the
        // named-fn-visible home the Any gate exists for (an
        // uninitialized `static x;` types "any" with an undefined
        // init, and every `__sm_` method write lands on it).
        && (!name.starts_with("__")
            || name.starts_with("__ccmk_")
            || name.starts_with("__sf_")
            // …and the resolver's re-export face bindings plus the
            // 423-01 deconflict mangles (rotation 426):
            // `__reex_<ns>_<face>` / `__m<k>_<name>` are minted, but
            // what they hold is the user's export, and the readers —
            // namespace objects opened in user fn bodies — are the
            // user's too. Same category as the ES5 class binding
            // below.
            || name.starts_with("__reex_")
            || crate::ssa_lower_inner::body_passes::strip_module_mangle(name) != name
            // …and the ES5 class binding (rotation 417): the name is
            // minted, but what it holds is the user's class, and the
            // named fns that read it are the user's too. See
            // `capturing_classes::is_es5_class_binding`.
            || crate::ast::capturing_classes::is_es5_class_binding(name)
            // …and the sloppy-goal implicit globals (goal-triage
            // family third member): USER names — the sputnik corpus
            // spells them `__x` — whose hoisted `var` the
            // `sloppy_implicit_globals` pass synthesized; a named-fn
            // body writing one needs the global slot.
            || ast.sloppy_implicit_global_names.contains(name)))
}

/// Whether a MUTABLE refcounted binding still promotes to a data
/// global (the `mutable_promote` gate, extracted verbatim rotation
/// 297).
///
/// K.6 — mutable Obj/Arr globals promote; the historical writeback
/// concern is retired. For Arr, B1 fixed the cell across growth:
/// `push` / `set_any_grow` realloc only the spilled data buffer
/// behind ARR_DATA_PTR_OFF and return the same cell
/// (`__torajs_arr_push` in torajs-arr grow.rs), so hidden mutation
/// through method calls needs no slot writeback — the push global
/// lane already ships on that invariant ("B1 — cell fixed across
/// grow; global-slot write-back retired", ssa_lower_call_arr_push).
/// Whole-binding reassignment rides the Assign-Ident global lane's
/// drop-old/store-new. Mutable primitive Copy globals stay promoted
/// (K.3 / globals-001 depends on it). Mutable Str globals promote
/// ONLY behind the named-fn-refs gate (chunk 558): strings have no
/// in-place mutation methods, so the writeback concern doesn't exist
/// and the Assign-Ident path handles drop-old/store-new — but the
/// slot's sole purpose is named-fn visibility, and unconditional
/// promotion drags await-init / borrow-shaped-init bindings
/// (`let s: string = await p`) into the K.4 fresh-heap-init
/// requirement they don't meet. No named-fn reader/writer → keep the
/// main-local home (its scope-drop walk already owns cleanup).
/// Chunk 730 (RFC 20260709-closure-global) — mutable Closure globals
/// promote behind the same gate: a closure's env is opaque to user
/// code (no in-place mutation surface), assignment is the only
/// mutation face and the Assign-Ident lane owns drop-old/store-new.
/// Chunk 740 — closure-captured bindings promote too: the capture
/// filter resolves the name to the global for reads AND the lifted
/// body's writes take the same Assign-Ident global lane, so the slot
/// IS the single home (the old env-copy snapshot disagreed with ES
/// shared-binding semantics). Chunk 809 — mutable Any globals promote
/// behind the same gate: assignment is the only mutation face this
/// lane owns (drop-old/box-new in the Assign-Ident lane), and member
/// writes route through the runtime any-member helpers on the loaded
/// box. RFC 20260725 (own-field-write follow-up) — mutable struct
/// globals promote behind the same gate: a struct cell's field write
/// is an in-place fixed-offset store (no reallocating method surface
/// exists on Obj), so the K.6 writeback concern doesn't apply, and
/// whole-binding reassignment rides the Assign-Ident global lane's
/// drop-old/store-new like Str/Closure/Any. Symbol rides the Str
/// profile: no in-place mutation methods exist, so assignment
/// (drop-old/store-new in the Assign-Ident lane) is the only
/// mutation face. Rotation 514 — the rest of the refcounted family
/// (Map / Set / Date / RegExp / Promise / BigInt / Weak* / Substr /
/// the iterators) joins on the Arr argument, which is the general
/// one: their mutation surface writes THROUGH the cell — a Map's
/// `set` grows storage behind its own pointer, a Date's `setTime`
/// and a RegExp's `lastIndex` are fixed-offset stores — so the cell
/// stays put and there is nothing to write back, while whole-binding
/// reassignment rides the Assign-Ident lane like every arm above.
/// Keeping them out is what made the supported gate disagree with
/// the checker for every `let` spelled with one.
/// r290 — a closure-captured Any binding joins the
/// gate: the hoisted-var `: any` shape's init is the Uninit sentinel
/// the Any lane already digests, so the K.4 fresh-heap-init concern
/// that keeps the concrete refcounted types on the named-fn gate
/// does not apply.
fn mutable_promotes(
    ty: &Type,
    name: &str,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
) -> bool {
    ty.is_refcounted()
        && (binding_refs.named_fn_refs.contains(name)
            || (*ty == Type::Any && binding_refs.closure_captured.contains(name)))
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
