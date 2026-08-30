//! `Stmt::LetDecl` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 156).
//!
//! Pre-extract this arm was 789 LOC inline inside `lower_stmt` —
//! the final big arm left after chunks 145-155 cleaned the rest.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! `let v[: T] = init` lowering with multiple fast-paths:
//!
//! 1. **T-19.d** — `let X: T = await Bun.file(p).json()`: typed
//!    JSON parse inlined with `fs_read_file_sync` + `lower_json_parse`.
//! 2. **T-02** — `let v: T = JSON.parse(text)`: caller-driven typed
//!    parse (number→F64 widening for "number" annotations, container
//!    widening via `widen_container_ty`); fresh-owned text Str drop.
//! 3. **T-09.c** — `let o: Pair = Object.fromEntries(es)`: per-field
//!    unfold from `Array<Array<Any>>` entries into struct slots
//!    (untag per field type, rc_inc heap-typed values).
//! 4. **K.3 / K.4** — top-level module global (main fn + name in
//!    `globals`): emit GlobalRef + Store, with K.6 empty-array fast
//!    path and refcount fresh-init requirement.
//! 5. **M2 Phase B Stage 4** — `let f = global_fn`: FnSig slot with
//!    FnAddr store.
//! 6. **General path** — `let v[: T] = init`:
//!    - Parse annotation (number→F64 if num_f64_slots says so;
//!      container widening).
//!    - Empty `[]` literal w/o annotation → Array<Any> default
//!      (P0.10 — interned ArrId for sibling pushes).
//!    - **Alias-init detection** (Member / Index / cross-scope
//!      Ident) — moved=true so end-of-scope drop skips; underlying
//!      owner is the canonical dropper.
//!    - **11-A2-a stack-alloc hint** for ObjectLit binding inits
//!      not in `escape_obj_lets` (ObjectLit arm consumes via
//!      `.take()` + escape-safety decided at runtime layout).
//!    - **T-10.c / T-11** Arr<Any> alloc dispatch.
//!    - **P3.2** `let x: any = { ... }` → `lower_dynobj_init`
//!      directly (bypass struct alloc); `let x: any = [...]` →
//!      `lower_array_any_literal` (mirrors P3.2 ObjectLit dynobj path).
//!    - **CPython/Swift-ARC** share-vs-consume — same-scope `let t = s`
//!      of a refcounted local emits `rc_inc` instead of consume.
//!    - Boundary coercions: i64→f64; concrete→Any via
//!      `box_to_any_from_expr`; type-ann inference (Type::Void
//!      placeholder); Substr widening (Str/Substr per init).
//!    - **T-15.g.5** escape-captured Copy let: heap capture-box
//!      (16B = rc + value), F64/Bool bit-cast to i64.
//!    - Shadowing: capture prior LocalInfo into shadow_stack when
//!      outer-scope name is reinstated on this scope's close.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
// Conversion crossings live in the convert sibling; the re-export
// keeps the pre-split call face (`ssa_lower_stmt_let_decl::…`) for
// the return-boundary and global-lane callers.
pub(crate) use crate::ssa_lower_stmt_let_decl_convert::{
    maybe_any_to_typed_scalar, maybe_arr_any_to_typed,
};
// Both halves of the ownership question the pipeline below asks about
// an init — does the binding borrow, and if not does it owe a share —
// live in `ssa_lower_stmt_let_decl_share`.

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mutable: bool,
) {
    // The specialized lanes, in order. Sibling (chunk of rotation 212)
    // — see its doc for why the chain moved out rather than up.
    if crate::ssa_lower_stmt_let_decl_dispatch::try_sub_siblings(ctx, name, type_ann, init, mutable)
    {
        return;
    }
    // General path. Stage helpers live in
    // [`crate::ssa_lower_stmt_let_decl_general`] (chunk 764).
    let mut ty =
        crate::ssa_lower_stmt_let_decl_general::initial_let_ty(ctx, name, type_ann, init, mutable);
    // RC-4 F1c — defineProperty receiver: unannotated ObjectLit
    // binding lowers through the P3.2 dynobj-init lane (`any`),
    // mirroring the checker's dynobj_degraded typing, so the define
    // write-back can rebind the slot.
    //
    // 2026-07-16 (rotation 121 chunk 5-followup) — mirror the
    // checker's undef-field-in-struct widen at `check_stmt_let_decl`
    // (`struct_has_undef_field`). Independent SSA-side compute so
    // the two sides can't drift; keys off the checker-stashed
    // `expr_types` (which is the checker's source-of-truth for the
    // init expression's type — same map the boxing sites read).
    let init_struct_has_undef = matches!(
        ctx.expr_types.get(&init),
        Some(crate::check::Type::Struct(fs))
            if fs.iter().any(|(_, ft)| matches!(ft, crate::check::Type::Undefined))
    );
    if type_ann.is_none()
        && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        && (ctx.dynobj_degraded.contains(&init) || init_struct_has_undef)
    {
        ty = Type::Any;
    }
    // RFC 20260804-mutable-let-widen — cross-family reassigned
    // binding lowers as an Any slot from declaration, mirroring
    // `check_stmt_let_decl`'s widen off the same shared set.
    if type_ann.is_none() && ctx.cross_type_widened.contains(&init) {
        ty = Type::Any;
    }
    crate::ssa_lower_stmt_let_decl_general::record_binding_flags(ctx, name, type_ann, init);
    let cur_depth = ctx.scope_stack.len() - 1;
    let is_alias_init = crate::ssa_lower_stmt_let_decl_share::init_is_alias(ctx, init, cur_depth);
    let stack_alloc_hinted = matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        && !ctx.escape_obj_lets.contains(name);
    if stack_alloc_hinted {
        ctx.let_stack_alloc_hint = Some(name.to_string());
    }
    // Chunk 780 — pin the annotated struct layout for a direct
    // ObjectLit init so resolve_objlit_layout doesn't first-match a
    // same-shaped layout registered under a different declared type
    // (see the field doc on `let_declared_obj_layout`).
    if let Type::Obj(sid) = ty
        && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
    {
        ctx.let_declared_obj_layout = Some(sid);
    }
    let init_val = lower_let_init_val(ctx, ty, init);
    ctx.let_stack_alloc_hint = None;
    ctx.let_declared_obj_layout = None;
    // Chunk 637 — the alias classification above ran BEFORE the init
    // lowering, so it can't see a Member read whose owned-receiver
    // lowering detached the result (`const v = mk(i).s` — see
    // `ssa_lower_member::lower`). Re-check: a detached result is
    // owned by this binding, which must keep its stake and drop at
    // scope end (probe l16o: the alias path stranded the field's
    // +1, 25.6 MB churn vs 6.4 MB flat).
    let is_alias_init = is_alias_init && !ctx.owned_member_reads.contains(&init);
    // Chunk 698 — the mirror of the chunk-621 crossing below: an
    // `Array<Any>` value bound to a typed `Array<T>` annotation
    // (`const a: number[] = Array.from(set)`). The typed raw-slot
    // readers would misdecode NaN-box bits as element values
    // (silent-wrong), so decode-copy into a fresh typed block at
    // the assign boundary (a mismatched slot is a catchable
    // TypeError); every downstream read stays on the typed fast
    // path. The conversion runs BEFORE the ownership bookkeeping:
    // whatever the init's shape, the binding now owns a fresh cell
    // (no alias, no share-inc), while the source keeps its layout
    // and its stake untouched (the helper always copies; an owned
    // temp init releases through release_owned_temp).
    let (init_val, converted) = maybe_arr_any_to_typed(ctx, ty, init, init_val);
    // The scalar sibling of the crossing above. `const x: number = t`
    // (`t: any`) reaches here with an Any operand and an I64/F64 slot;
    // without a decode the NaN-box bits ARE the stored number
    // (`{v:10}` read back as -562949953421302), and a Str slot reads
    // them as a pointer. The two other Any→typed boundaries already
    // decode — assignment (`ssa_lower_assign_ident`'s
    // permissible-coercion table) and the call-arg lane — so these are
    // the same rows, on the binding.
    let (init_val, scalar_converted) = maybe_any_to_typed_scalar(ctx, type_ann, ty, init, init_val);
    let converted = converted || scalar_converted;
    let is_alias_init = is_alias_init && !converted;
    // RFC 20260705 ledger #2 (chunk 563) — a concrete value boxed into
    // an `any` slot is ALWAYS owned by the slot: `anyv_box_from_pair`
    // transfers one reference (NaN-box contract), and every any-slot
    // consumer (assign drop-old, scope drop) releases one. Borrow-shape
    // inits (Ident / Member / container Index) therefore take +1 before
    // the box; owned shapes (Call / BinOp / string-indexing view)
    // transfer their fresh reference. The old consume path moved the
    // source binding's single stake into the box (double-free when
    // boxed twice); the old alias path left the box borrowing a stake
    // the slot's drop-old then stole (UAF).
    let boxed_any = ty == Type::Any && ctx.operand_ty(&init_val) != Type::Any;
    if !is_alias_init {
        let pre_ty = ctx.operand_ty(&init_val);
        let shares = crate::ssa_lower_stmt_let_decl_share::init_shares_source_stake(
            ctx, init, pre_ty, ty, converted,
        );
        // No consume on the non-share side: every shape reaching it is
        // a no-op for the move-walk (Copy / non-local Ident / non-Ident
        // expr — local refcounted Idents all take the share arm) —
        // chunk 572 removed the dead marker.
        if shares {
            ctx.emit_rc_inc(init_val.clone());
        }
    }
    // §10.4.6 — a module namespace binding always lowers through the
    // dynobj lane (`dynobj_degrade` puts its init there unconditionally
    // for exactly this), so the fresh object can be given the exotic
    // attributes right here, before anything is able to read them:
    // null prototype, non-extensible, non-configurable entries, and
    // the `@@toStringTag` own entry. An importer whose every use was
    // a static member read never reaches this — `module_ns_members`
    // retargeted those, and the elision drops the binding entirely.
    if ty == Type::Any && ctx.ast.namespace_bindings.contains_key(name) {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.module_ns_finalize, vec![init_val.clone()]),
            Type::Void,
            None,
        );
    }
    finalize_and_bind(
        ctx,
        name,
        type_ann,
        init,
        ty,
        init_val,
        is_alias_init,
        boxed_any,
        cur_depth,
    );
}

/// Post-init-val finalization + slot bind: F64/I64 coerce → box-to-any
/// (with the RFC 20260705 ledger #2 inc-then-box on a borrow-shape init)
/// → un-annotated `let`'s I64→F64 widen when `num_f64_slots` proved the
/// slot is F64-only → the RFC 20260707 chunk 621 Arr<Any> kind-mark for
/// a typed-array init shared into an any-elem annotation → Str/Substr
/// + Arr<Str>/Arr<Substr> annotation-widen convergence → hand off to
/// `bind_let_slot`. Kept in one flat sequence (not a chain of `let
/// init_val = ...` shadow rebindings) so the ownership hand-off through
/// each stage stays inspectable.
fn finalize_and_bind(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mut ty: Type,
    mut init_val: Operand,
    is_alias_init: bool,
    boxed_any: bool,
    cur_depth: usize,
) {
    if ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        init_val = ctx.coerce_to_f64(init_val);
    }
    if boxed_any {
        if ctx.operand_ty(&init_val).is_refcounted() && !ctx.expr_transfers_ownership(init) {
            ctx.emit_rc_inc(init_val.clone());
        }
        init_val = ctx.box_to_any_from_expr(init, init_val);
    }
    if type_ann.is_none() {
        ty = ctx.operand_ty(&init_val);
        if ty == Type::I64
            && ctx
                .num_f64_slots
                .slot_is_f64(&ctx.num_width_local_key(name))
        {
            ty = Type::F64;
            init_val = ctx.coerce_to_f64(init_val);
        }
    }
    let init_ty = ctx.operand_ty(&init_val);
    // RFC 20260707 chunk 621 — a typed array shared into an `any[]`
    // binding (T-11 container widen) keeps its raw-slot layout; mark
    // its elem kind so the kind-aware Arr<Any> readers can rebox the
    // raw slots (mirror of the assign_member field-store mark). Gated
    // on the binding's Any elem so typed→typed decls pay no call.
    if let (Type::Arr(ann_id), Type::Arr(_)) = (&ty, &init_ty)
        && ctx.arr_layouts[ann_id.0 as usize] == Type::Any
    {
        ctx.emit_arr_mark_kind(&init_val);
    }
    if ty == Type::Str && init_ty == Type::Substr {
        ty = Type::Substr;
    } else if let Type::Arr(init_id) = init_ty
        && ctx.arr_layouts[init_id.0 as usize] == Type::Substr
        && ctx.ast.let_owned_elem_inits.contains(&init)
        && ctx.expr_transfers_ownership(init)
    {
        // A split product something in scope writes an owned value
        // INTO (`a.push(v)` / `a[i] = v` / …) or hands on as a bare
        // value (`f(a)` / `let b = a` / `o.f = a` / …) cannot stay an
        // `Arr<Substr>`: the slot cannot take an owned cell, every
        // reader decodes by the view layout, and a receiver is typed
        // by its own annotation. The fresh product is materialized in
        // place — each view becomes an owned string, its parent
        // reference handed back — and the binding is typed `Arr<Str>`,
        // where the mutators already store any string shape and every
        // receiver agrees (rotation 468, plan-state 467-01). Only a
        // product this binding owns outright is converted in place;
        // an alias's owner was itself listed and converted.
        ty = Type::Arr(crate::ssa_lower::intern_arr_layout(
            ctx.arr_layouts,
            Type::Str,
        ));
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_substr_materialize_owned, vec![init_val]),
            ty,
            None,
        );
        init_val = Operand::Value(owned);
    } else if let (Type::Arr(ann_id), Type::Arr(init_id)) = (ty, init_ty)
        && ctx.arr_layouts[ann_id.0 as usize] == Type::Str
        && ctx.arr_layouts[init_id.0 as usize] == Type::Substr
    {
        ty = init_ty;
    }
    crate::ssa_lower_stmt_let_decl_general::bind_let_slot(
        ctx,
        name,
        ty,
        init_val,
        is_alias_init,
        boxed_any,
        cur_depth,
    );
}

/// general-path init-value dispatch: empty `[]` alloc (needs an array
/// annotation), Arr<Any> literal, P3.2 `any`-annotated ObjectLit → dynobj
/// / Array → any-literal, else plain `lower_expr`
fn lower_let_init_val(ctx: &mut LowerCtx, ty: Type, init: ExprId) -> Operand {
    // Chunk 618 — a void-call init evaluates for effect and binds
    // undefined (the checker typed the binding Undefined;
    // ConstPtrNull is the same representation an uninitialized
    // `let w;` binding carries). Keyed on the CHECKER type of the
    // init expr — the ssa `ty` param uses Void as its no-annotation
    // sentinel and can't discriminate. Chunk 806 — user-fn void
    // calls now type Undefined (general_call), so a Call-shaped
    // Undefined init takes this arm too; a plain `= undefined` init
    // keeps its existing path.
    if matches!(ctx.expr_types.get(&init), Some(crate::check::Type::Void))
        || (matches!(
            ctx.expr_types.get(&init),
            Some(crate::check::Type::Undefined)
        ) && matches!(
            ctx.ast.get_expr(init),
            Expr::Call { .. } | Expr::OptCall { .. }
        ))
    {
        let _ = ctx.lower_expr(init);
        return Operand::ConstPtrNull;
    }
    // RFC 20260710 C1, at the LET-INIT position — a plain
    // `= undefined` for a sentinel-capable slot binds that width's
    // undefined cell rather than the NULL its literal lowers to. The
    // struct-FIELD position has always done this; `const a: string |
    // undefined = undefined` did not, so it bound null and answered
    // `typeof a === "string"`, `a === undefined` false. None for Any
    // (which boxes to ANY_UNDEF on its own), for the scalar widths,
    // and for the un-annotated Void sentinel — those lanes unchanged.
    if matches!(
        ctx.expr_types.get(&init),
        Some(crate::check::Type::Undefined)
    ) && let Some(sentinel) = ctx.str_undef_sentinel_for(ty)
    {
        return sentinel;
    }
    if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
        // An `any`-annotated `[]` falls through to the any-literal
        // arm below (mints an empty Arr<Any> — chunk-809 any-ann
        // family, rotation 73 L3b; the checker admits it since the
        // same cut).
        && ty != Type::Any
    {
        if !matches!(ty, Type::Arr(_)) {
            panic!("ssa-lower: empty `[]` literal needs an array type annotation; got {ty:?}");
        }
        let alloc_fn = if let Type::Arr(arr_id) = ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            ty,
            None,
        );
        Operand::Value(v)
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && let Type::Arr(arr_id) = ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else if ty == Type::Any && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. }) {
        ctx.lower_dynobj_init(init)
    } else if ty == Type::Any
        && let Expr::Array(els) = ctx.ast.get_expr(init)
    {
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        ctx.lower_expr(init)
    }
}
