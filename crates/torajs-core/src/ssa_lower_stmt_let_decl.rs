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
use crate::ssa_lower::{LocalInfo, LowerCtx, intern_arr_layout};
use crate::ssa_lower_parse_type::parse_type;

pub(crate) fn lower(ctx: &mut LowerCtx, name: &str, type_ann: Option<&String>, init: ExprId) {
    // T-19.d — `let X: T = await Bun.file(p).json()`. Sub-sibling.
    if crate::ssa_lower_stmt_let_decl_bun_json::try_lower(ctx, name, type_ann, init) {
        return;
    }
    // T-02 — `let v: T = JSON.parse(text)`. Sub-sibling.
    if crate::ssa_lower_stmt_let_decl_json_parse::try_lower(ctx, name, type_ann, init) {
        return;
    }
    // T-09.c — `let o: Pair = Object.fromEntries(es)`. Sub-sibling.
    if crate::ssa_lower_stmt_let_decl_fromentries::try_lower(ctx, name, type_ann, init) {
        return;
    }
    // K.3 / K.4 — top-level data global.
    if try_lower_global_let(ctx, name, init) {
        return;
    }
    // M2 Phase B Stage 4 — `let f = global_fn`.
    if try_lower_fn_addr_let(ctx, name, init) {
        return;
    }
    // General path.
    let mut ty = if type_ann.is_some() {
        let parsed = parse_type(
            type_ann.map(|s| s.as_str()),
            ctx.aliases,
            ctx.arr_layouts,
            ctx.fn_sigs,
            ctx.generic_struct_decls,
            ctx.struct_layouts,
            ctx.inst_memo,
        );
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
    };
    // RC-4 F1c — defineProperty receiver: unannotated ObjectLit
    // binding lowers through the P3.2 dynobj-init lane (`any`),
    // mirroring the checker's dynobj_degraded typing, so the define
    // write-back can rebind the slot.
    if type_ann.is_none()
        && matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        && ctx.dynobj_degraded.contains(name)
    {
        ty = Type::Any;
    }
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
    // RFC 20260707 residual chunk — record string-index let-inits
    // (`const c = s[i]`) and their aliases: the Substr slot may
    // hold the undefined sentinel (OOB read), so the inline
    // str-eq-with-literal fast path declines to the identity-aware
    // runtime compare.
    if crate::ssa_lower_nullable_guard::is_undefable_substr_source(ctx, init) {
        ctx.undefable_substr_lets.insert(name.to_string());
    }
    let cur_depth = ctx.scope_stack.len() - 1;
    let is_alias_init = match ctx.ast.get_expr(init) {
        // L3b #15 residual (chunk 561) — string indexing (`s[i]` on
        // a Str/Substr receiver) emits a FRESH standalone Substr
        // view (str_char_at / substr_slice, rc=1), not a borrow of
        // an element slot: marking it moved skipped the scope drop
        // and leaked 32B per binding. Array/map/any indexing keeps
        // the alias path (element loads borrow the container).
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
        }
        Expr::Member { .. } => true,
        Expr::Ident(src) => ctx
            .locals
            .get(src)
            .map(|info| info.scope_depth < cur_depth)
            .unwrap_or(false),
        _ => false,
    };
    let stack_alloc_hinted = matches!(ctx.ast.get_expr(init), Expr::ObjectLit { .. })
        && !ctx.escape_obj_lets.contains(name);
    if stack_alloc_hinted {
        ctx.let_stack_alloc_hint = Some(name.to_string());
    }
    let init_val = lower_let_init_val(ctx, ty, init);
    ctx.let_stack_alloc_hint = None;
    // Chunk 637 — the alias classification above ran BEFORE the init
    // lowering, so it can't see a Member read whose owned-receiver
    // lowering detached the result (`const v = mk(i).s` — see
    // `ssa_lower_member::lower`). Re-check: a detached result is
    // owned by this binding, which must keep its stake and drop at
    // scope end (probe l16o: the alias path stranded the field's
    // +1, 25.6 MB churn vs 6.4 MB flat).
    let is_alias_init = is_alias_init && !ctx.owned_member_reads.contains(&init);
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
        let shares = if let Expr::Ident(src) = ctx.ast.get_expr(init) {
            ctx.locals.contains_key(src)
                && pre_ty.is_refcounted()
                && !(ty == Type::Any && pre_ty != Type::Any)
        } else {
            false
        };
        // No consume on the non-share side: every shape reaching it is
        // a no-op for the move-walk (Copy / non-local Ident / non-Ident
        // expr — local refcounted Idents all take the share arm) —
        // chunk 572 removed the dead marker.
        if shares {
            ctx.emit_rc_inc(init_val.clone());
        }
    }
    let init_val = if ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        ctx.coerce_to_f64(init_val)
    } else {
        init_val
    };
    let init_val = if boxed_any {
        if ctx.operand_ty(&init_val).is_refcounted() && !ctx.expr_transfers_ownership(init) {
            ctx.emit_rc_inc(init_val.clone());
        }
        ctx.box_to_any_from_expr(init, init_val)
    } else {
        init_val
    };
    let init_val = if type_ann.is_none() {
        ty = ctx.operand_ty(&init_val);
        if ty == Type::I64
            && ctx
                .num_f64_slots
                .slot_is_f64(&ctx.num_width_local_key(name))
        {
            ty = Type::F64;
            ctx.coerce_to_f64(init_val)
        } else {
            init_val
        }
    } else {
        init_val
    };
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
    } else if let (Type::Arr(ann_id), Type::Arr(init_id)) = (ty, init_ty)
        && ctx.arr_layouts[ann_id.0 as usize] == Type::Str
        && ctx.arr_layouts[init_id.0 as usize] == Type::Substr
    {
        ty = init_ty;
    }
    let escape_captured = ty.is_copy() && ctx.escape_captured_lets.contains(name);
    let slot = if escape_captured {
        ctx.emit_capture_boxed(ty, init_val)
    } else {
        let slot = ctx.binding_slot_alloca(ty, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(init_val, Operand::Value(slot), 0),
        );
        slot
    };
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
            borrowed: is_alias_init && !boxed_any,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
}

/// K.3 / K.4 — top-level data global: main fn + name in `globals` emits
/// GlobalRef + Store (K.6 empty-array fast path; refcount slots require
/// fresh-heap init). Returns false when `name` is not a module global.
fn try_lower_global_let(ctx: &mut LowerCtx, name: &str, init: ExprId) -> bool {
    if !ctx.is_main_fn {
        return false;
    }
    let Some(slot_ty) = ctx.globals.get(name).copied() else {
        return false;
    };
    let init_val = if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
        && matches!(slot_ty, Type::Arr(_))
    {
        let alloc_fn = if let Type::Arr(arr_id) = slot_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            slot_ty,
            None,
        );
        Operand::Value(v)
    } else if let Expr::Array(els) = ctx.ast.get_expr(init)
        && let Type::Arr(arr_id) = slot_ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        ctx.lower_expr(init)
    };
    if slot_ty.is_refcounted() {
        let init_is_borrow = matches!(
            ctx.ast.get_expr(init),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
        if init_is_borrow {
            // RFC 20260707 chunk 627 — Ident-shaped init (alias of
            // another binding, a guaranteed +0 borrow): the global
            // slot takes its own stake; the source binding keeps its
            // own. Only same-type and Arr↔Arr (T-11 container widen)
            // aliases are admitted — an Any↔concrete mismatch needs a
            // box/unbox station and stays loud rather than storing
            // wrong-repr bits. Member/Index inits stay loud too:
            // their ownership shape is lane-dependent (arr_index_get
            // answers +1), so a blanket inc would double-count.
            let got = ctx.operand_ty(&init_val);
            let ident_alias = matches!(ctx.ast.get_expr(init), Expr::Ident(_));
            let compatible =
                got == slot_ty || matches!((slot_ty, got), (Type::Arr(_), Type::Arr(_)));
            if !ident_alias || !compatible {
                panic!(
                    "ssa-lower: K.4 refcount global `{name}` requires fresh-heap or same-type ident-alias init; this init shape is not yet supported"
                );
            }
            ctx.emit_owned_result_inc(init_val, got);
        }
    }
    let coerced = if slot_ty == Type::F64 && ctx.operand_ty(&init_val) == Type::I64 {
        ctx.coerce_to_f64(init_val)
    } else {
        init_val
    };
    // RFC 20260707 chunk 627 — a typed array stored into an Arr<Any>
    // global slot (T-11 widen) marks the block's elem kind for the
    // kind-aware Arr<Any> readers (621 let-decl general-path mirror;
    // self-gating no-op for non-array / Arr<Any> values).
    ctx.emit_arr_mark_kind(&coerced);
    let ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(name.to_string()),
        Type::Ptr,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(coerced, Operand::Value(ptr), 0),
    );
    true
}

/// M2 Phase B Stage 4 — `let f = global_fn`: FnSig slot with FnAddr
/// store. Returns false when init is not a bare global-fn ident.
fn try_lower_fn_addr_let(ctx: &mut LowerCtx, name: &str, init: ExprId) -> bool {
    let Expr::Ident(src_name) = ctx.ast.get_expr(init) else {
        return false;
    };
    if ctx.locals.get(src_name).is_some() {
        return false;
    }
    let Some(fid) = ctx.fn_table.get(src_name).copied() else {
        return false;
    };
    let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied() else {
        return false;
    };
    let ty = Type::FnSig(sig_id);
    let slot = ctx.binding_slot_alloca(ty, name);
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::FnAddr(fid), ty, None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    let cur_depth = ctx.scope_stack.len() - 1;
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            moved: false,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
    true
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
    // sentinel and can't discriminate.
    if matches!(ctx.expr_types.get(&init), Some(crate::check::Type::Void)) {
        let _ = ctx.lower_expr(init);
        return Operand::ConstPtrNull;
    }
    if let Expr::Array(els) = ctx.ast.get_expr(init)
        && els.is_empty()
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
