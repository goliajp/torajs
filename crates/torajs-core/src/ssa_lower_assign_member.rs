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
//! 7. **Type::Obj** (struct receiver) — the accessor-setter direct
//!    call (`P8.2`, args through the `arg_conv` contract) and the
//!    direct struct field store both live in
//!    [`crate::ssa_lower_assign_member_field`].

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

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
        // A fresh owned receiver box (inline as-cast, member-read
        // chain) releases inside the set tail; an Ident borrow keeps
        // its binding's stake (and rides the write-back instead).
        let recv_owned = ctx.expr_transfers_ownership(obj);
        return crate::ssa_lower_assign_member_any::lower_dynobj_assign(
            ctx, obj_val, &field, value, &obj_ident, recv_owned,
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
    lower_obj_assign(ctx, obj, obj_val, sid, &field, value)
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
    // ES §10.4.2.5 — scalar element types keep the truncate helper;
    // every other element type (Any / Str / Arr / ...) routes to the
    // full resize helper (per-slot release on truncate, undefined
    // fill on Array<Any> grow — RFC 20260712 backlog item landed).
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
        ctx.intrinsics.arr_set_length_any
    };
    let argv = vec![obj_val, tag, val_op];
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
    obj: ExprId,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    if let Some(v) = crate::ssa_lower_assign_member_field::try_lower_setter_call(
        ctx, obj, obj_val, sid, field, value,
    ) {
        return v;
    }
    if let Some(v) =
        crate::ssa_lower_assign_member_objlit::try_lower(ctx, obj_val, sid, field, value)
    {
        return v;
    }
    crate::ssa_lower_assign_member_field::lower_struct_field_store(ctx, obj_val, sid, field, value)
}
