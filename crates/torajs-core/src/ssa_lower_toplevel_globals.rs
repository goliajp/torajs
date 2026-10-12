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
//!   - missing type annotation without a named-fn reader — `let Y =
//!     computeValue()` without `: T` keeps the K.1 behavior of being a
//!     main-fn local (named-fn read errors with "unknown ident"). A
//!     binding a named fn reads takes the slot of the spelling the
//!     checker registered it from (`Checker::global_spellings`).
//!   - annotated primitive bindings that NO named-fn body references
//!     (the localize gate) — the slot exists solely for named-fn
//!     visibility, so a main-only binding keeps the local home and
//!     the slot-promotion family dissolves it into registers.
//!   - mutable refcount-typed annotations (Arr / Obj) — hidden
//!     mutation through method calls (`xs.push(v)`) would need
//!     writeback to the slot (K.6, not yet landed).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, GenericStructDecls, PropKey, Stmt};
use crate::num_width::SlotKey;
use crate::ssa::Type;
use crate::ssa_lower::parse_type;

mod gate;
mod write_through;
use gate::{mutable_promotes, slot_type_supported, stays_main_local};
use write_through::binding_written_through;
pub(crate) use write_through::init_is_static_string_split;

/// `global_spellings` is the checker's record of the annotation spelling
/// each un-annotated binding registered from (`Checker::global_spellings`);
/// the slot parses the identical string.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_toplevel_globals(
    ast: &Ast,
    cell_fns: &HashSet<&str>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    global_spellings: &HashMap<String, String>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, crate::ssa::StructId>,
    num_f64_slots: &crate::num_width::WidthTable,
) -> HashMap<String, Type> {
    let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
    // A binding a namespace object names ahead of its declaration is
    // read through a cell over its slot (`ns_live_exports`), so it
    // needs a slot even when every direct read could inline it.
    let ns_live = crate::ast_refs::ns_live_exports(ast);
    let mut globals: HashMap<String, Type> = HashMap::new();
    let widened = crate::let_widen::collect_cross_type_widen_inits(ast, Some(expr_types));
    let degraded = crate::dynobj_degrade::collect_dynobj_degraded_inits(ast);
    // Multi-flattened walk (rotation 230) — the same flat view the
    // checker's pass_2 walks.
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
            if init_is_inline_literal
                && !*mutable
                && type_ann.as_deref() != Some("any")
                && !ns_live.contains(name)
            {
                continue;
            }
            // Slot type. With an annotation, parse it; without one,
            // parse the spelling the checker registered the binding
            // from. Both answer `None` for shapes that keep the K.1
            // main-local behavior. An initializer whose own layout
            // carries accessor slots keeps that layout; the annotation
            // only gated admission in the checker
            // (`check_accessor_objlit_slot`). Parsing it here would
            // hand the global a data field where the value has a
            // getter closure, so such a binding takes the spelling
            // path the same literal takes unannotated.
            //
            // The parse decides, not the annotation's presence: only
            // an `Obj` / `Arr` slot denotes a concrete LAYOUT that a
            // literal's accessor slots can conflict with. An `any`
            // annotation is a box whose dynamic member lane serves
            // accessors already — and setting it aside here loses the
            // global entirely (`const t: any = { get then() { … } }`
            // became `unknown ident t`, which the conformance gate
            // caught and no probe would have).
            let mut from_spelling = |written: Option<&str>| {
                let (ann, written) = match written {
                    Some(ann) => (ann, true),
                    None => (global_spellings.get(name)?.as_str(), false),
                };
                slot_ty_from_ann(
                    ann,
                    written,
                    name,
                    *init,
                    ast,
                    cell_fns,
                    (&degraded, &widened),
                    expr_types,
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                    num_f64_slots,
                )
            };
            let slot_ty = match type_ann {
                Some(ann) => {
                    let annotated = from_spelling(Some(ann));
                    let accessor_layout = matches!(annotated, Some(Type::Obj(_) | Type::Arr(_)))
                        && expr_types.get(init).is_some_and(
                            crate::check_accessor_objlit_slot::layout_carries_accessor,
                        );
                    if accessor_layout {
                        from_spelling(None)
                    } else {
                        annotated
                    }
                }
                None => from_spelling(None),
            };
            let Some(ty) = slot_ty else {
                continue;
            };
            let ty = match type_ann {
                Some(ann) => crate::check_stmt_let_decl::any_layout_slot::value_repr(
                    &ty,
                    (Some(ann.as_str()), *init),
                    ast,
                    expr_types,
                    widened.contains(init),
                    arr_layouts,
                )
                .unwrap_or(ty),
                None => ty,
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
            if stays_main_local(&ty, name, &binding_refs) {
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

/// Slot type for a top-level binding from an annotation spelling —
/// `written` in the source, or synthesized by the checker
/// (`Checker::global_spellings`). `None` keeps the binding main-local.
///
/// A synthesized fn spelling widens by the slot's own key, not by the
/// spelling's nominal class: with no annotation written, the binding
/// never joined that class — its widths live on the slot key's
/// `__ret` / `__p{i}` projections, glued to the lifted fn's keys by
/// the let site's `fn_value_flow`. Querying the class would answer
/// stale parse widths, and the env-first CallIndirect would read a
/// floated callee's d0 ret off x0. A synthesized spelling also comes
/// from an ALIAS of a lifted arrow (`const c = k`), which holds the
/// same env cell the arrow mints and takes the same Closure slot.
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
fn slot_ty_from_ann(
    ann: &str,
    written: bool,
    name: &str,
    init: ExprId,
    ast: &Ast,
    cell_fns: &HashSet<&str>,
    (degraded, widened): (&HashSet<ExprId>, &HashSet<ExprId>),
    expr_types: &HashMap<ExprId, crate::check::Type>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
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
    let key = SlotKey::Global(name.to_string());
    let parsed = if !written && matches!(parsed, Type::FnSig(_) | Type::Closure(_)) {
        crate::ssa_lower_container_width::widen_fn_sig_by_key(
            parsed,
            &key,
            num_f64_slots,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        )
    } else {
        crate::ssa_lower_container_width::widen_container_ty(
            parsed,
            Some(ann),
            &key,
            num_f64_slots,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        )
    };
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
    // a call to a user fn returning an env cell (`= mk(1)`) or a read
    // of an env cell field (`= src.g`) holds that cell too
    let calls_cell_fn = matches!(ast.get_expr(peeled), Expr::Call { callee, .. }
        if matches!(ast.get_expr(*callee), Expr::Ident(f) if cell_fns.contains(f.as_str())));
    let closure_shaped = init_is_lifted_arrow
        || calls_cell_fn
        || crate::ast_refs::closure_field_read_ann(ast, peeled, degraded, widened).is_some()
        || (!written && crate::ast_refs::closure_alias_fn_canon(ast, peeled).is_some());
    // Any other Any-typed init (an any-bound ident, an any member read)
    // carries the cell shape too, but K.4's global-init store lane has
    // no Any→Closure conversion: it stays main-local, where the fn-local
    // let lane converts (`initial_let_ty` mirror); a named-fn read of it
    // is the recorded main-local unknown-ident limitation.
    if matches!(parsed, Type::FnSig(_))
        && !init_is_lifted_arrow
        && matches!(expr_types.get(&init), Some(crate::check::Type::Any))
    {
        return None;
    }
    let parsed = match parsed {
        Type::FnSig(sig) if !ann.contains("__rest(") && closure_shaped => Type::Closure(sig),
        Type::Closure(sig) if ann.contains("__rest(") && closure_shaped => Type::Closure(sig),
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
