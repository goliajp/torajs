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
//!   → store new at same offset` via struct layout walk.
//! - **Index** (`arr[i]++`) — `load arr+offset → compute new →
//!   store new at same addr` via T-13.5 head-aware
//!   `emit_arr_slot_byte_offset` (11-A1 non-deque peek fast
//!   path).
//!
//! Type is always Number (typecheck enforced); the incr op must
//! match the slot width — `FAdd`/`FSub` on `ConstF64(1.0)` for
//! f64 slots, `Add`/`Sub` on `ConstI64(1)` for i64 (codegen
//! dispatches register class by op kind, so an int Add with an
//! f64 result is an invalid SSA shape).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::PostIncr` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, target: ExprId, is_inc: bool) -> Operand {
    match ctx.ast.get_expr(target).clone() {
        Expr::Ident(name) => lower_ident(ctx, name, is_inc),
        Expr::Member { obj, name: field } => lower_member(ctx, obj, field, is_inc),
        Expr::Index { obj, index } => lower_index(ctx, obj, index, is_inc),
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

fn lower_member(ctx: &mut LowerCtx<'_>, obj: ExprId, field: String, is_inc: bool) -> Operand {
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);
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

fn lower_index(ctx: &mut LowerCtx<'_>, obj: ExprId, index: ExprId, is_inc: bool) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    let arr_ty = ctx.operand_ty(&arr_val);
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
