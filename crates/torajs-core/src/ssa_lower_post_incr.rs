//! `Expr::PostIncr { target, is_inc }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-74
//! of the decomp (chunks 1-73 = ... + `Expr::Nullish` 4-layer
//! dispatch).
//!
//! JS spec: yield the OLD value, then mutate. Three target shapes
//! mirror Assign:
//!
//! - **Ident** (`x++`) — `load slot → compute new → store new`.
//!   V3-18 m1.h.26 global slot path: when the target ident is a
//!   registered mutable global (e.g. a static class field after
//!   the ClassDecl desugar), `GlobalRef + Load + Store` matches
//!   the read / Assign paths and keeps post-incr working for
//!   `Counter.value++`.
//! - **Member** (`obj.field++`) — `load obj+offset → compute new
//!   → store new at same offset` via struct layout walk. The
//!   `xs.length++/--` array form routes the store through the
//!   §10.4.2.5 resize helper instead (see [`lower_arr_length`]).
//! - **Index** (`arr[i]++`) — `load arr+offset → compute new →
//!   store new at same addr` via T-13.5 head-aware
//!   `emit_arr_slot_byte_offset` (11-A1 non-deque peek fast
//!   path).
//!
//! On the typed lanes the incr op must match the slot width —
//! `FAdd`/`FSub` on `ConstF64(1.0)` for f64 slots, `Add`/`Sub` on
//! `ConstI64(1)` for i64 (codegen dispatches register class by op
//! kind, so an int Add with an f64 result is an invalid SSA shape).
//!
//! An `any` slot cannot be stepped inline at all: §13.4.4.1 puts a
//! ToNumeric between the load and the add, which must run exactly once
//! and which picks the numeric domain — BigInt or Number — the step
//! happens in. Both Ident shapes hand the slot pointer to one runtime
//! call instead ([`lower_any_slot`]). Member on an `any` RECEIVER has
//! no slot pointer to hand over — the store must go back through the
//! member-set kernel with its accessor / refusal semantics — so
//! [`lower_member_any`] composes three existing lanes: the
//! accessor-aware any-member read (GetV), the value-shaped
//! ToNumeric + step kernel (`anyv_incr_value`), and the tag-gated
//! member-set tail. Index on an `any` element stays loud (registered).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::PostIncr` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, target: ExprId, is_inc: bool) -> Operand {
    match ctx.ast.get_expr(target).clone() {
        // RFC 20260730-undeclared-ident, write position — §13.4.4.1
        // step 1 is GetValue on the target, so an update expression
        // over a marked unresolvable name raises the read-side
        // ReferenceError before any numeric step; nothing else here
        // is reachable.
        Expr::Ident(name) if ctx.ast.undeclared_reads.contains_key(&target) => {
            let name_str = ctx.intern_string_literal(&name);
            let raiser = ctx.intrinsics.throw_reference_error_name;
            let cur_block = ctx.cur_block;
            ctx.f.append_void(
                cur_block,
                InstKind::Call(raiser, vec![Operand::Value(name_str)]),
            );
            ctx.emit_throw_check(None);
            return Operand::ConstPtrNull;
        }
        Expr::Ident(name) => lower_ident(ctx, name, is_inc),
        Expr::Member { obj, name: field } => lower_member(ctx, eid, target, obj, field, is_inc),
        Expr::Index { obj, index } => lower_index(ctx, eid, obj, index, is_inc),
        other => panic!("ssa-lower: post-incr target shape not supported: {other:?}"),
    }
}

fn lower_ident(ctx: &mut LowerCtx<'_>, name: String, is_inc: bool) -> Operand {
    if ctx.locals.get(&name).is_none()
        && let Some(slot_ty) = ctx.globals.get(&name).copied()
    {
        return lower_ident_global(ctx, name, slot_ty, is_inc);
    }
    let info = match ctx.locals.get(&name) {
        Some(i) => *i,
        None => panic!("ssa-lower: post-incr on unknown ident `{name}`"),
    };
    if info.ty == Type::Any {
        return lower_any_slot(ctx, Operand::Value(info.slot), is_inc);
    }
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    let (one, op) = incr_op_one(info.ty, is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        info.ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), Operand::Value(info.slot), 0),
    );
    Operand::Value(old)
}

fn lower_ident_global(
    ctx: &mut LowerCtx<'_>,
    name: String,
    slot_ty: Type,
    is_inc: bool,
) -> Operand {
    let cur_block = ctx.cur_block;
    let ptr = ctx
        .f
        .append_inst(cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
    if slot_ty == Type::Any {
        return lower_any_slot(ctx, Operand::Value(ptr), is_inc);
    }
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(slot_ty, Operand::Value(ptr), 0),
        slot_ty,
        None,
    );
    let (one, op) = incr_op_one(slot_ty, is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        slot_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), Operand::Value(ptr), 0),
    );
    Operand::Value(old)
}

/// The `any`-slot lane — one call carrying the whole §13.4.4.1 step
/// (load, ToNumeric, add one in the operand's own numeric domain,
/// release the replaced value, store) and answering the coerced old
/// value. See `Intrinsics::any_incr_slot` for why the runtime takes
/// the slot pointer instead of the loaded value.
fn lower_any_slot(ctx: &mut LowerCtx<'_>, slot: Operand, is_inc: bool) -> Operand {
    let fid = ctx.intrinsics.any_incr_slot;
    let flag = Operand::ConstI64(if is_inc { 1 } else { 0 });
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![slot, flag]),
        Type::Any,
        None,
    );
    Operand::Value(old)
}

fn lower_member(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    obj: ExprId,
    field: String,
    is_inc: bool,
) -> Operand {
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);
    // Length-write knife (rotation 270) — `xs.length--` / `++` on an
    // array: load the len, step, route the store through the
    // §10.4.2.5 resize helper (mirrors `lower_arr_length_assign`;
    // truncation releases slots, grow fills holes), yield the old
    // value. Surfaced by LiveLength arguments bodies decrementing
    // `arguments.length`, but serves plain arrays the same.
    if matches!(obj_ty, Type::Arr(_)) && field == "length" {
        return lower_arr_length(ctx, obj, obj_val, obj_ty, is_inc);
    }
    if matches!(obj_ty, Type::Any) {
        return lower_member_any(ctx, eid, target, obj, obj_val, &field, is_inc);
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        other => panic!("ssa-lower: post-incr field on non-obj {other:?}"),
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let (idx, field_ty) = layout
        .iter()
        .enumerate()
        .find_map(|(i, (fname, fty))| {
            if fname == &field {
                Some((i, *fty))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("ssa-lower: struct {sid:?} has no field `{field}`"));
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    let (one, op) = incr_op_one(field_ty, is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        field_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), obj_val, offset),
    );
    Operand::Value(old)
}

/// The `any`-receiver lane of [`lower_member`] — §13.4.4.1 over a
/// receiver whose shape is a runtime question. Three existing lanes
/// compose: the accessor-aware any-member read answers the current
/// value (GetV), `anyv_incr_value` runs ToNumeric exactly once and
/// steps in the operand's own numeric domain, and the tag-gated
/// member-set tail stores the new value with the full setter /
/// refusal semantics (write-back included for a named Any receiver).
/// The expression answers the coerced OLD value — owned, so the
/// PostIncr eid joins `owned_member_reads` and consumers release it.
fn lower_member_any(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    obj: ExprId,
    obj_val: Operand,
    field: &str,
    is_inc: bool,
) -> Operand {
    let old_box =
        crate::ssa_lower_any_member::lower_any_member_read(ctx, target, obj_val.clone(), field);
    let old_slot = ctx.alloca(Type::Any, Some("__incr_old"));
    let flag = Operand::ConstI64(if is_inc { 1 } else { 0 });
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_incr_value,
            vec![old_box.clone(), flag, Operand::Value(old_slot)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(old_box, Type::Any);
    // Owned unbox (tag, payload+1) feeds the member-set kernel; the
    // surplus box stake releases after the store — the
    // `lower_dynobj_assign_any_payload` contract.
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![Operand::Value(new_v)]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_unbox_value_owned,
            vec![Operand::Value(new_v)],
        ),
        Type::I64,
        None,
    );
    let obj_ident = match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => Some(n.clone()),
        _ => None,
    };
    let recv_owned = ctx.expr_transfers_ownership(obj);
    crate::ssa_lower_assign_member_any::emit_any_member_set(
        ctx,
        obj_val,
        field,
        Operand::Value(tag_v),
        Operand::Value(val_v),
        &obj_ident,
        recv_owned,
    );
    ctx.emit_drop_value(Operand::Value(new_v), Type::Any);
    ctx.owned_member_reads.insert(eid);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(old_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(old)
}

/// The Arr-length lane of [`lower_member`] — old len loads inline
/// (read-path twin at `ssa_lower_member_typed_props`), the stepped
/// value stores through the same helper split as
/// `lower_arr_length_assign` (scalar elem kinds take the truncate
/// helper, refcounted kinds the full resize; tag 2 = raw i64 pair,
/// `define_length` precedent). The helper can throw (readonly
/// length / invalid value), hence the throw check.
fn lower_arr_length(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    is_inc: bool,
) -> Operand {
    crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &obj_val);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, obj_val.clone(), crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let (one, op) = incr_op_one(Type::I64, is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        Type::I64,
        None,
    );
    let elem_ty = match obj_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        _ => unreachable!("gated on Type::Arr above"),
    };
    let helper = if matches!(elem_ty, Type::I64 | Type::F64 | Type::Bool) {
        ctx.intrinsics.arr_set_length_truncate_scalar
    } else {
        ctx.intrinsics.arr_set_length_any
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            helper,
            vec![obj_val, Operand::ConstI64(2), Operand::Value(new_v)],
        ),
    );
    ctx.emit_throw_check(None);
    Operand::Value(old)
}

fn lower_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    is_inc: bool,
) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    let arr_ty = ctx.operand_ty(&arr_val);
    // Cluster #4 blade 2 — an `any` (or boxed-struct) receiver's
    // keyed update rides the keyed kernels, mirroring
    // [`lower_member_any`]. A null/undefined receiver raises inside
    // the keyed read.
    if matches!(arr_ty, Type::Any | Type::Obj(_)) {
        let recv = if matches!(arr_ty, Type::Obj(_)) {
            ctx.box_to_any(arr_val)
        } else {
            arr_val
        };
        return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
            ctx, eid, index, recv, is_inc,
        );
    }
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        other => panic!("ssa-lower: post-incr index on non-array {other:?}"),
    };
    let idx_val = ctx.lower_index_operand(index);
    let (offset_base, offset) =
        ctx.emit_arr_slot_byte_offset(arr_val.clone(), idx_val, 3, is_non_deque);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(elem_ty, offset_base.clone(), offset.clone()),
        elem_ty,
        None,
    );
    let (one, op) = incr_op_one(elem_ty, is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        elem_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::StoreDyn(Operand::Value(new_v), offset_base.clone(), offset),
    );
    Operand::Value(old)
}

fn incr_op_one(slot_ty: Type, is_inc: bool) -> (Operand, SsaBinOp) {
    if slot_ty == Type::F64 {
        let f = if is_inc {
            SsaBinOp::FAdd
        } else {
            SsaBinOp::FSub
        };
        (Operand::ConstF64(1.0), f)
    } else {
        let i = if is_inc { SsaBinOp::Add } else { SsaBinOp::Sub };
        (Operand::ConstI64(1), i)
    }
}
