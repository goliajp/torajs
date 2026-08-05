//! General-path helpers of [`crate::ssa_lower_stmt_let_decl::lower`]
//! — chunk 764 extraction (the general path had grown to 321 fn
//! lines past the 200-line limit as RFC chunks stacked re-repr /
//! sentinel-registry / capture-box stages onto it). Three stages,
//! bodies verbatim:
//!
//! 1. [`initial_let_ty`] — annotation parse + fn-repr re-tag +
//!    number-slot F64 promote + container widen (or the empty-`[]`
//!    / no-annotation defaults).
//! 2. [`record_binding_flags`] — the nullable / undefable /
//!    variadic source-shape registries keyed by binding name.
//! 3. [`bind_let_slot`] — capture-box vs plain alloca slot, shadow
//!    capture, `LocalInfo` insert, scope push.

use crate::ast::Expr;
use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx, intern_arr_layout};
use crate::ssa_lower_parse_type::parse_type;

/// Stage 1 — resolve the binding's initial SSA type from its
/// annotation (parse + re-repr + width promote + container widen),
/// or the empty-`[]` Arr<Any> default, or the Void placeholder for
/// annotation-less inits (inferred after lowering).
pub(crate) fn initial_let_ty(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mutable: bool,
) -> Type {
    if type_ann.is_some() {
        let parsed = parse_type(
            type_ann.map(|s| s.as_str()),
            ctx.aliases,
            ctx.arr_layouts,
            ctx.fn_sigs,
            ctx.generic_struct_decls,
            ctx.struct_layouts,
            ctx.inst_memo,
        );
        // L3b #4 — a MUTABLE fn-typed local re-reprs Closure (the
        // toplevel-globals K.3b decision, fn-local mirror):
        // reassignment stores env-carrying arrows, which a FnSig
        // (direct-dispatch) slot can't hold. Chunk 734 — an IMMUTABLE
        // fn-typed local whose init is a lifted arrow (`const f: ()
        // => number = () => x`) re-reprs the same way (728fix
        // toplevel gate, fn-local mirror): every arrow lifts to
        // Expr::Closure and mints an env cell, which a FnSig slot
        // dispatches as a raw code address (SIGBUS). Immutable
        // named-fn inits never reach here (fn_addr_let claims them
        // above — direct dispatch preserved). Variadic anns parse to
        // Closure already and never take this arm.
        let parsed = match parsed {
            Type::FnSig(sig)
                if mutable || matches!(ctx.ast.get_expr(init), Expr::Closure { .. }) =>
            {
                Type::Closure(sig)
            }
            t => t,
        };
        if parsed == Type::I64
            && type_ann.map(|s| s.as_str()) == Some("number")
            && ctx
                .num_f64_slots
                .slot_is_f64(&ctx.num_width_local_key(name))
        {
            Type::F64
        } else {
            crate::ssa_lower_container_width::widen_container_ty(
                parsed,
                type_ann.map(|s| s.as_str()),
                &ctx.num_width_local_key(name),
                ctx.num_f64_slots,
                ctx.arr_layouts,
                ctx.struct_layouts,
                ctx.fn_sigs,
            )
        }
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
    {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        Type::Arr(arr_id)
    } else {
        Type::Void
    }
}

/// Stage 2 — record the binding in the source-shape registries its
/// init matches, so downstream consumers (member/index loads,
/// typeof / strict-eq / print, call dispatch) pick the guarded or
/// boxed lanes.
pub(crate) fn record_binding_flags(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) {
    // RC-4 F1a — record exec/match-shaped let-inits so member/index
    // loads on this binding emit the null guard (miss is null per
    // the checker's Nullable<Array<Str>> typing).
    if crate::ssa_lower_nullable_guard::is_nullable_arr_source(ctx, init) {
        ctx.nullable_arr_lets.insert(name.to_string());
    }
    // RFC 20260707-undefined-sentinel-repr chunk 1 — record
    // element-load-off-nullable-arr let-inits (`const c = m[1]`)
    // and their aliases so `.length` loads and the inline
    // str-eq-with-literal fast path treat the binding as
    // possibly-NULL (missed capture slot).
    if crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, init) {
        ctx.nullable_str_lets.insert(name.to_string());
    }
    // A WRITTEN `T | null` / `T | undefined` says the same thing the
    // init-shape probes above infer, and says it directly. Every
    // registry here reads the INIT; the annotation — the one piece of
    // evidence the programmer stated outright — went unread, so
    // `const a: string | undefined = undefined` answered
    // `typeof a === "string"`. Which set follows the slot width, the
    // same split the parameter twin makes.
    if type_ann.is_some_and(|a| a.starts_with("__nullable(")) {
        match crate::ssa_lower_parse_type::parse_type(
            type_ann.map(|s| s.as_str()),
            &ctx.aliases,
            &mut ctx.arr_layouts,
            &mut ctx.fn_sigs,
            ctx.generic_struct_decls,
            &mut ctx.struct_layouts,
            &mut ctx.inst_memo,
        ) {
            Type::Str => {
                ctx.nullable_str_lets.insert(name.to_string());
            }
            Type::Substr => {
                ctx.undefable_substr_lets.insert(name.to_string());
            }
            t if t.spells_undef_with_generic_cell() => {
                ctx.undefable_heap_lets.insert(name.to_string());
            }
            _ => {}
        }
    }
    // RFC 20260708-typed-arr-oob-read chunk 2 — record `number[]`
    // index-read let-inits (`const x = a[i]`) so typeof / strict-eq
    // / nullish / print / box consumers treat the binding as
    // possibly holding the undefined-NaN sentinel.
    if crate::ssa_lower_nullable_guard::is_undef_f64_source(ctx, init) {
        ctx.undefable_f64_lets.insert(name.to_string());
    }
    // RFC 20260708-variadic — a rest-tail fn-type annotation
    // (`const t: (...xs: E[]) => R = …`) routes calls through the
    // boxed dual entry, same as a variadic-annotated param. An
    // UNannotated binding whose init the checker typed rest-tail
    // (`const f = mk()` where `mk(): (...args: E[]) => R` — chunk 3)
    // registers the same way: the SSA slot only carries the fixed-
    // prefix sig, so the static lane would mismatch the stored
    // closure's real arity.
    if type_ann.is_some_and(|a| a.contains("__rest("))
        || matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Function(ps, _))
                if matches!(ps.last(), Some(crate::check::Type::Rest(_)))
        )
    {
        ctx.variadic_locals.insert(name.to_string());
    }
    // RFC 20260719-ns-static-value-reify — a binding initialized
    // from a namespace-static VALUE read (`const m = Math.max`)
    // routes its calls through the boxed dual entry: the minted
    // cell's fn_addr is the typed-slot boundary throw, its boxed
    // entry the real receiver-less dispatcher (and the dispatcher
    // gives min/max-style variadic spec semantics for free). The id
    // registers alongside so the `.length` member fold answers the
    // table's spec length, not the checker-sig param count.
    if let Some(id) = ns_static_member_init_id(ctx, init) {
        ctx.variadic_locals.insert(name.to_string());
        ctx.ns_static_locals.insert(name.to_string(), id);
    }
    // RFC 20260725-str-method-value-reify — a binding initialized
    // from a String-receiver method VALUE read (`const m = s.slice`)
    // routes its calls the same way: the minted mid-cell's fn_addr
    // is the typed-slot boundary throw, its boxed entry the spec
    // this-undefined TypeError. The mid registers alongside so the
    // `.length` / `.name` folds answer the spec meta row.
    if let Some(mid_fam) = builtin_mv_member_init_mid(ctx, init) {
        ctx.variadic_locals.insert(name.to_string());
        ctx.builtin_mv_locals.insert(name.to_string(), mid_fam);
    }
    // RFC 20260707 residual chunk — record string-index let-inits
    // (`const c = s[i]`) and their aliases: the Substr slot may
    // hold the undefined sentinel (OOB read), so the inline
    // str-eq-with-literal fast path declines to the identity-aware
    // runtime compare.
    if crate::ssa_lower_nullable_guard::is_undefable_substr_source(ctx, init) {
        ctx.undefable_substr_lets.insert(name.to_string());
    }
    // RFC 20260722-find-miss chunk B — record heap-elem
    // find/findLast let-inits (and Nullable heap-source aliases) so
    // member reads / typeof / strict-eq / box consumers on the
    // binding take the undef-cell-aware lanes.
    if crate::ssa_lower_nullable_guard::is_undefable_heap_source(ctx, init) {
        ctx.undefable_heap_lets.insert(name.to_string());
    }
}

/// The table id when `init` is a namespace-static VALUE read
/// (`Math.max`) — the same three-part gate the member lowering mints
/// under: table hit + obj typed `Type::Object` (a user binding
/// shadowing `Math` never matches) + member typed `Function`.
pub(crate) fn ns_static_member_init_id(ctx: &LowerCtx, init: ExprId) -> Option<i64> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(init) else {
        return None;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    let id = torajs_rc::ns_static_id(ns, name);
    if id != torajs_rc::NS_STATIC_UNKNOWN
        && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Object(_)))
        && matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Function(..))
        )
    {
        Some(id)
    } else {
        None
    }
}

/// The `(mid, proto family)` when `init` is a primitive-receiver
/// builtin method VALUE read (`s.slice` / `n.toString`) — the same
/// gate the member lowering mints under: the receiver types to a
/// prototype family + member typed `Function` + the name interns to
/// a mid with a spec meta row.
pub(crate) fn builtin_mv_member_init_mid(ctx: &LowerCtx, init: ExprId) -> Option<(i64, i64)> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(init) else {
        return None;
    };
    let fam = crate::ssa_lower_member::mv_family_of_checker_ty(ctx.expr_types.get(obj)?)?;
    if !matches!(
        ctx.expr_types.get(&init),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    let mid = torajs_rc::any_method_id(name);
    if mid == torajs_rc::ANY_METHOD_UNKNOWN || torajs_rc::any_method_meta(mid).is_none() {
        return None;
    }
    Some((mid, fam))
}

/// Stage 3 — materialize the binding's slot (capture box for
/// escape-captured bindings, plain alloca otherwise), capture any
/// shadowed outer binding, and register the `LocalInfo`.
pub(crate) fn bind_let_slot(
    ctx: &mut LowerCtx,
    name: &str,
    ty: Type,
    init_val: Operand,
    is_alias_init: bool,
    boxed_any: bool,
    cur_depth: usize,
) {
    // A box for this name went up before the declaration ran, because a
    // closure earlier in the list captured it; the envs already hold
    // that box, so the value belongs in it rather than in a slot of
    // this frame's own.
    if let Some(sites) = ctx.forward_capture_boxes.remove(name) {
        fill_forward_box(ctx, name, ty, init_val, is_alias_init, boxed_any, sites);
        return;
    }
    // RFC 20260710 — a MUTATED non-Copy captured binding promotes to
    // a capture box so every closure shares the live binding (ES
    // §9.1) instead of an env-owns snapshot. Never-written captures
    // keep the snapshot (indistinguishable from sharing, zero
    // indirection); Any / Substr slots are recorded C4 boundaries
    // (NaN-box / view-cell release paths differ from the universal
    // heap drop).
    let boxed_noncopy = !ty.is_copy()
        && ty != Type::Substr
        && ctx.escape_captured_lets.contains(name)
        && ctx.mutated_captured_lets.contains(name);
    let escape_captured =
        (ty.is_copy() && ctx.escape_captured_lets.contains(name)) || boxed_noncopy;
    let slot = if escape_captured {
        if boxed_noncopy && is_alias_init && !boxed_any {
            // An alias-shape init carries no stake of its own — the
            // box takes a fresh share (the source binding keeps its;
            // Any payloads take the NaN-box-aware inc).
            ctx.emit_owned_result_inc(init_val.clone(), ty);
        }
        ctx.emit_capture_boxed(ty, init_val)
    } else {
        let slot = ctx.binding_slot_alloca(ty, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(init_val, Operand::Value(slot), 0),
        );
        slot
    };
    if boxed_noncopy {
        ctx.boxed_noncopy_lets.insert(name.to_string());
    }
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
        top_shadow.push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            // A boxed-any binding owns the box's stake regardless of
            // the init's alias shape — it must reach the scope-close
            // drop walk (chunk 563).
            moved: (is_alias_init && !boxed_any) || escape_captured,
            // A boxed non-Copy binding owns its box stake even when
            // the init was an alias (the box inc'd its own share).
            borrowed: is_alias_init && !boxed_any && !boxed_noncopy,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
}

/// Settle a binding whose capture box went up before its declaration:
/// store the value into the waiting box and replace the provisional
/// type — on the local, and on every env slot that already took it.
///
/// `open_box` did the registration (shadow save, scope push, the
/// `boxed_noncopy_lets` mark that makes both the capture write and the
/// scope-close release take the box path), so none of that repeats
/// here; only the type was left open, because an un-annotated init has
/// none until it lowers.
///
/// Ownership is the same ledger as the boxed arm above: the box holds
/// one stake and this frame holds the box. A value the init produced
/// outright transfers straight in; an alias-shaped one leaves its
/// source's stake alone and the box takes a share of its own.
fn fill_forward_box(
    ctx: &mut LowerCtx,
    name: &str,
    ty: Type,
    init_val: Operand,
    is_alias_init: bool,
    boxed_any: bool,
    sites: Vec<(String, usize)>,
) {
    if !ty.is_copy() && is_alias_init && !boxed_any {
        ctx.emit_owned_result_inc(init_val.clone(), ty);
    }
    // The frame releases a Copy payload's box through its own list,
    // keyed on the slot being Copy and escape-captured. `open_box` had
    // to mark the name on the non-Copy list to get the capture write
    // to take the byref path, and could not yet tell which payload
    // this would be; leaving it on both lists releases the same box
    // twice. A Copy slot answers byref on its own, so the mark has
    // done its work and comes off.
    if ty.is_copy() {
        ctx.boxed_noncopy_lets.remove(name);
    }
    let slot = ctx
        .locals
        .get(name)
        .expect("forward capture box registered the binding")
        .slot;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(init_val, Operand::Value(slot), 0),
    );
    for (fn_name, idx) in sites {
        if let Some(caps) = ctx.closure_captures.get_mut(&fn_name) {
            caps[idx].1 = ty;
        }
    }
    if let Some(info) = ctx.locals.get_mut(name) {
        info.ty = ty;
    }
}
