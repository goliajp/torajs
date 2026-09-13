//! The two admission gates of [`super::collect_toplevel_globals`] —
//! which slot types the data-global machinery carries at all, and
//! when a MUTABLE refcounted binding promotes. Split from the parent
//! (file-size hard limit, RFC 20260912-global-var-cell); bodies verbatim.

use crate::ast::Ast;
use crate::ssa::Type;

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
pub(super) fn slot_type_supported(
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
        // The first clause asks PROVENANCE, not spelling: a `__` name
        // the PROGRAM wrote is an ordinary user binding and promotes
        // like any other (`let __o: any = {}` read from a named fn was
        // a hard `ssa-lower: unknown ident` before rotation 573 — the
        // same program with the prefix removed compiled). The clauses
        // under it are a different question: those names ARE minted,
        // but what they HOLD is the user's, so they promote too.
        && (!crate::ast::is_synthesized_dunder(ast, name)
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
            // …and the namespace object's `default` field binding
            // (rotation 628): `__nsdefault_<ns>` is minted by the
            // resolver, holds the user's default export, and its
            // reader is the dynamic-import dispatcher — a named fn.
            || name.starts_with("__nsdefault_")
            // …and the demoted built-in global (628-07): `__gshadow_<G>`
            // is minted by `builtin_global_assign`, holds the user's
            // rebinding of `eval` / `Array` / …, and the writers and
            // readers are the user's named fns (`function f() { eval =
            // 42 }`, `new Function("eval = 42;")`). Its seed is `let
            // __gshadow_G: any = G`, so the annotation path above
            // types the slot; only this provenance clause stood
            // between it and the named-fn-visible home.
            || name.starts_with("__gshadow_")
            // …and a dynamic-import candidate's evaluation memo
            // (rotation 645): `__dyn_memo_c<k>` is minted by the
            // resolver, holds the module's [[Status]] / [[EvaluationError]]
            // (§16.2.1.5.3), and its only reader and writer is the
            // candidate's `__dyn_eval_c<k>` — a named fn.
            || name.starts_with("__dyn_memo_")
            || crate::ssa_lower_inner::body_passes_fn_name::strip_module_mangle(name) != name
            // …and the ES5 class binding (rotation 417): the name is
            // minted, but what it holds is the user's class, and the
            // named fns that read it are the user's too. See
            // `capturing_classes::is_es5_class_binding`.
            || crate::ast::capturing_classes::is_es5_class_binding(name)))
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
pub(super) fn mutable_promotes(
    ty: &Type,
    name: &str,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
) -> bool {
    ty.is_refcounted()
        && (binding_refs.named_fn_refs.contains(name)
            || (*ty == Type::Any && binding_refs.closure_captured.contains(name)))
}
