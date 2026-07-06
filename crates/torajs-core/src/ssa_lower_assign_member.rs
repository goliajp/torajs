//! `Expr::Assign { target: Expr::Member { obj, name: field }, value }`
//! lowering pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Assign` match arm as chunk-80 of the decomp.
//!
//! Dispatch ladder (each path returns the assigned value as the
//! expression result, modulo the dynobj / Closure / Arr-prop paths
//! that return `ConstI64(0)` to match the legacy in-line emit shape):
//!
//! 1. **Type::Any** (`P3.2`, tag-gated per RFC 20260704 C4+) — pack
//!    RHS as `(tag, value)` and call `__torajs_any_member_set`, which
//!    dispatches on the receiver's heap tag (DynObj set / RegExp
//!    lastIndex / Arr expando; anything else a catchable TypeError —
//!    never a blind dynobj-layout write). Nested Type::Any payload
//!    routes through `any_payload_rc_inc`. Frozen / non-writable
//!    write throws via `emit_throw_check`. Post-resize relocation
//!    write-back rides the AnyValue slot, re-stored to a plain-Ident
//!    receiver's local.
//! 2. **Type::Closure** (`T-27`) — `f.x = v` writes to the closure's
//!    lazy `props_dynobj` at `CLOSURE_PROPS_OFF` via `fn_props_set`.
//! 3. **Type::FnSig** (`T-27.b`) — top-level FnDecl routes through
//!    the `fnprops` side table keyed by fn pointer.
//! 4. **Type::Arr + field=="length"** (`S133-3`) — spec §9.4.2.4 length
//!    setter: route to `arr_set_length_validate` (refcount elem types)
//!    or `arr_set_length_truncate_scalar` (i64/f64/bool elements).
//! 5. **Type::Arr** (other field, `T-29`) — `arr.x = v` writes to the
//!    array's side-table `props_dynobj` (keyed by ptr) via
//!    `arrprops_set`. `arr_drop` / `arr_drop_any` drop_entry hook
//!    cleans the bucket at refcount == 0.
//! 6. **Type::RegExp + field=="lastIndex"** (`P9.4`) — coerce RHS to
//!    f64 (uncoerced-store spec shape; ToLength happens where the
//!    regex kernels consume it), call `regex_set_last_index`, return
//!    the value as the expression result (mirrors a field store).
//! 7. **Type::Obj** (struct receiver) — two sub-paths:
//!    - **Accessor setter** (`P8.2`) — desugar_classes renamed the
//!      setter to `__cm_<C>__<name>_set` and registered
//!      `accessor_setters[(C, name)] → fn_name`. Emit Call with
//!      `[obj_val, value]`, widen i64→f64 when the setter param is
//!      f64 (F2-fix). Return the original value.
//!    - **Direct struct field store** — locate `(idx, field_ty)` in
//!      the struct layout, emit `obj_check_not_frozen` (`T-09.d`
//!      v0.4.0 frozen guard) with a real TypeError throw-check
//!      (`P7.4-frozen`), evaluate RHS with `V3-06` empty-array-from-
//!      field-Type override, width-align (`W4`) i64→f64 if the
//!      field is f64, drop the old non-Copy field value, then Store.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, obj: ExprId, field: String, value: ExprId) -> Operand {
    // Step 7d-A — capture the LHS variable name (if `obj` is a plain
    // Ident) so the Type::Any dynobj-set / dynobj_define paths below
    // can write the post-resize ptr back to the variable's storage
    // as a fresh NaN-box `AnyValue`.
    let obj_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        Some(n.clone())
    } else {
        None
    };
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);

    if matches!(obj_ty, Type::Any) {
        return crate::ssa_lower_assign_member_any::lower_dynobj_assign(
            ctx, obj_val, &field, value, &obj_ident,
        );
    }
    if matches!(obj_ty, Type::Closure(_)) {
        return lower_closure_props_assign(ctx, obj_val, &field, value);
    }
    if matches!(obj_ty, Type::FnSig(_)) {
        return lower_fnsig_props_assign(ctx, obj_val, &field, value);
    }
    if matches!(obj_ty, Type::Arr(_)) && field == "length" {
        return lower_arr_length_assign(ctx, obj_val, obj_ty, value);
    }
    if matches!(obj_ty, Type::Arr(_)) {
        return lower_arr_props_assign(ctx, obj_val, &field, value);
    }
    if obj_ty == Type::RegExp && field == "lastIndex" {
        return lower_regex_last_index_assign(ctx, obj_val, value);
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        other => panic!("ssa-lower: field assign on non-obj {other:?}"),
    };
    lower_obj_assign(ctx, obj_val, sid, &field, value)
}

fn lower_closure_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — storing into closure props is a SHARE: no consume.
    // box_to_tag_value mints the bucket's +1; a borrow-shape rhs
    // keeps the source binding's stake, an owned temp releases its
    // surplus reference after the store.
    let transfers = ctx.expr_transfers_ownership(value);
    let v_ty = ctx.operand_ty(&v_raw);
    let (tag, val_op) = ctx.box_to_tag_value(v_raw.clone());
    ctx.fn_props_set(obj_val, field, tag, val_op);
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_raw, v_ty);
    }
    Operand::ConstI64(0)
}

fn lower_fnsig_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — SHARE, mirror of the closure-props arm above.
    let transfers = ctx.expr_transfers_ownership(value);
    let v_ty = ctx.operand_ty(&v_raw);
    let (tag, val_op) = ctx.box_to_tag_value(v_raw.clone());
    let key_str = ctx.intern_string_literal(field);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.fnprops_set,
            vec![obj_val, Operand::Value(key_str), tag, val_op],
        ),
    );
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_raw, v_ty);
    }
    Operand::ConstI64(0)
}

fn lower_arr_length_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    obj_ty: Type,
    value: ExprId,
) -> Operand {
    // Chunk 566 — no consume: the rhs is a number (checker-gated),
    // so the historical consume was a Copy no-op; an Any-typed Ident
    // rhs must keep its stake (the validate helper only reads).
    let (tag, val_op) = ctx.lower_to_tag_value(value);
    // ES §10.4.2.5 step 4 — for non-refcounted scalar element types
    // we route to the truncate-aware helper that also writes
    // `len = N` when `N < oldLen`. Refcounted element types (Str /
    // Substr / Arr / ...) still go through validate-only until the
    // per-slot rc_dec truncate path lands.
    let elem_ty = if let Type::Arr(elem_arr_id) = obj_ty {
        Some(ctx.arr_layouts[elem_arr_id.0 as usize])
    } else {
        None
    };
    let truncate_scalar = matches!(
        elem_ty,
        Some(Type::I64) | Some(Type::F64) | Some(Type::Bool)
    );
    let helper = if truncate_scalar {
        ctx.intrinsics.arr_set_length_truncate_scalar
    } else {
        ctx.intrinsics.arr_set_length_validate
    };
    let argv = if truncate_scalar {
        vec![obj_val, tag, val_op]
    } else {
        vec![tag, val_op]
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(helper, argv));
    ctx.emit_throw_check(None);
    Operand::ConstI64(0)
}

fn lower_arr_props_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    // lower_to_tag_value keeps `undefined` ANY_UNDEF (plain pair
    // would collapse to null). Chunk 566 — SHARE: the bucket's +1
    // comes from box_to_tag_value inside; no consume, and an owned
    // temp releases its surplus reference after the store.
    let (tag, val_op, v_raw, v_ty) = ctx.lower_to_tag_value_raw(value);
    let transfers = ctx.expr_transfers_ownership(value);
    let key_str = ctx.intern_string_literal(field);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.arrprops_set,
            vec![obj_val, Operand::Value(key_str), tag, val_op],
        ),
    );
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_raw, v_ty);
    }
    Operand::ConstI64(0)
}

fn lower_regex_last_index_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — no consume: any_to_number only READS an Any rhs,
    // so a borrow-shape source keeps its stake; an owned Any temp's
    // box releases after the store below.
    let transfers = ctx.expr_transfers_ownership(value);
    // lastIndex is an ordinary data property — the store is
    // uncoerced (`r.lastIndex = 2.9` reads back 2.9); ToLength
    // happens at the regex kernels' consumption sites. An Any RHS
    // goes through ToNumber for the f64 slot.
    let v_ty = ctx.operand_ty(&v_raw);
    let v_keep = v_raw.clone();
    let v_f64 = if v_ty == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![v_raw]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(v_raw)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_set_last_index,
            vec![obj_val, v_f64.clone()],
        ),
    );
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_keep, v_ty);
    }
    v_f64
}

fn lower_obj_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    if let Some(v) = try_lower_setter_call(ctx, obj_val, sid, field, value) {
        return v;
    }
    lower_struct_field_store(ctx, obj_val, sid, field, value)
}

fn try_lower_setter_call(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Option<Operand> {
    // P8.2 — accessor write: `c.value = v` where C declares
    // `set value(n: T)`. desugar_classes renamed the setter's FnDecl
    // to `__cm_<C>__<name>_set` and recorded
    // `(C, name) → fn_name` in `ast.accessor_setters`. Emit a Call
    // to the setter with `[obj_val, value]` and return the value
    // (parallel to a normal Store which also evaluates to the
    // value). Skips the struct field lookup + Store path below.
    let mut setter_cname: Option<String> = None;
    for (n, ty) in ctx.aliases.iter() {
        if matches!(ty, Type::Obj(s) if s.0 == sid.0) && ctx.ast.class_parents.contains_key(n) {
            setter_cname = Some(n.clone());
            break;
        }
    }
    let cname = setter_cname?;
    let setter_fn = ctx
        .ast
        .accessor_setters
        .get(&(cname.clone(), field.to_string()))
        .cloned()?;
    let fid = ctx.fn_table.get(&setter_fn).copied()?;
    let v = ctx.lower_expr(value);
    // Chunk 566 — SHARE: no consume. The value passes to the setter
    // as a +0 borrow; the setter body's own field store takes the
    // field's +1 (struct-field share below), so a borrow-shape rhs
    // keeps the source binding's stake and an owned temp releases
    // its surplus after the call. Recorded edge (RFC 20260705
    // ledger): a non-storing setter + a consumer binding the assign
    // result reads a released temp — assign-result ownership is its
    // own lane.
    let transfers = ctx.expr_transfers_ownership(value);
    // F2-fix — the accessor arm bypasses the width-aware direct-call
    // coercion; an i64 value must widen to the setter's f64 param
    // (raw bits read as a denormal).
    let mut arg = v.clone();
    if let Some(sig_id) = ctx.fn_sig_ids.get(&fid).copied()
        && ctx.fn_sigs[sig_id.0 as usize].0.get(1) == Some(&Type::F64)
        && ctx.operand_ty(&arg) == Type::I64
    {
        arg = ctx.coerce_to_f64(arg);
    }
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Call(fid, vec![obj_val, arg]));
    ctx.emit_throw_check(Some(fid));
    let v_ty = ctx.operand_ty(&v);
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v.clone(), v_ty);
    }
    Some(v)
}

fn lower_struct_field_store(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let (idx, field_ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| {
            if fname == field {
                Some((i, *fty))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("ssa-lower: struct {sid:?} has no field `{field}`"));
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    // T-09.d (v0.4.0) — frozen mutation guard. Inline call to runtime
    // helper that panics with a TypeError-shaped message if the
    // object's universal heap header has the FROZEN bit set. Matches
    // bun's strict-mode throw on `Object.freeze(o); o.field = ...`.
    // ~3-cycle overhead on the unfrozen path (single load + and + cmp
    // + branch-not-taken after LLVM inlines the call body).
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.obj_check_not_frozen, vec![obj_val]),
    );
    // P7.4-frozen — obj_check_not_frozen now arms a real TypeError
    // (instead of process abort) when the target is frozen. Force
    // the throw-check here (intrinsic → emit_throw_check(Some) would
    // skip it) so it diverts to the try/catch or propagates BEFORE
    // the field store below — the illegal mutation must not happen.
    // Mirrors the a-2 dynobj writable=false pattern.
    ctx.emit_throw_check(None);
    // V3-06 — `this.kids = []` in a constructor. Mirrors the K.6
    // LetDecl-global path: empty array literals lack inferable
    // element type on their own, so we allocate from the field's
    // declared `Type::Arr` here. An Any-elem field takes the
    // `arr_alloc_any` variant — the plain alloc left FLAG_ARR_ANY
    // clear, so every runtime flag-dispatch consumer (drop walker,
    // cycle collector, any-boxed readers) treated the array as
    // typed (RFC 20260706 Phase C conviction: an any[] field cycle
    // was invisible to the collector, and its NaN-box cell elems
    // were never dec'd on drop).
    let v = if let Expr::Array(els) = ctx.ast.get_expr(value)
        && els.is_empty()
        && matches!(field_ty, Type::Arr(_))
    {
        let alloc_fn = if let Type::Arr(arr_id) = field_ty
            && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
        {
            ctx.intrinsics.arr_alloc_any
        } else {
            ctx.intrinsics.arr_alloc
        };
        let cur_block = ctx.cur_block;
        let alloc = ctx.f.append_inst(
            cur_block,
            InstKind::Call(alloc_fn, vec![Operand::ConstI64(0)]),
            field_ty,
            None,
        );
        Operand::Value(alloc)
    } else if let Expr::Array(els) = ctx.ast.get_expr(value)
        && let Type::Arr(arr_id) = field_ty
        && ctx.arr_layouts[arr_id.0 as usize] == Type::Any
    {
        // Chunk 614 — non-empty literal into an Any-elem field takes
        // the same annotation-consuming widen the LetDecl path has
        // (`lower_let_init_val`): without it the literal lowered
        // through the typed fast path, so the stored block never got
        // FLAG_ARR_ANY — the cycle collector's `is_visitable_arr`
        // said leaf, an `any[]` field cycle was invisible (obj root
        // buffered but the trial-deletion walk never crossed the
        // arr), and its raw scalar slots were NaN-box-misread by
        // every flag-dispatch consumer.
        let ids: Vec<ExprId> = els.clone();
        ctx.lower_array_any_literal(&ids)
    } else {
        let v = ctx.lower_expr(value);
        // Chunk 566 — a field store SHARES the rhs (TS has no move
        // semantics): a borrow-shape value takes +1 so the field
        // owns its stake while the source binding keeps its own —
        // the old consume let a re-assign's drop-old steal the
        // source's only ref (UAF, reuse-window probe-proven). Owned
        // temps keep transferring their fresh reference.
        let transfers = ctx.expr_transfers_ownership(value);
        // W4 — align the stored value with the field width (mirrors
        // the index-assign site; the reverse direction means the
        // width analysis missed this write).
        let v = match (field_ty, ctx.operand_ty(&v)) {
            (Type::F64, Type::I64) => ctx.coerce_to_f64(v),
            (Type::I64, Type::F64) => panic!(
                "ssa-lower: f64 value into i64 struct field `{field}` — \
                 container width analysis missed this write"
            ),
            _ => v,
        };
        let v_ty = ctx.operand_ty(&v);
        if !transfers && !v_ty.is_copy() {
            ctx.emit_rc_inc(v.clone());
        }
        v
    };
    // Drop the old field value if non-Copy.
    if !field_ty.is_copy() {
        let cur_block = ctx.cur_block;
        let old = ctx.f.append_inst(
            cur_block,
            InstKind::Load(field_ty, obj_val, offset),
            field_ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), field_ty);
    }
    // L3b #6 crash fix — mirror the ObjectLit field-store mark: a
    // typed Array entering a struct field must carry its elem kind
    // for the runtime walkers (inspect / cycle collector). No-op for
    // non-Arr fields. Chunk 621 — the chain now derives from the
    // value's own type inside the helper: an `any[]` field taking a
    // typed array (T-11 widen) marked chain 0 off the field type and
    // left the block invisible to the kind-aware readers.
    ctx.emit_arr_mark_kind(&v);
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(v, obj_val, offset));
    v
}
