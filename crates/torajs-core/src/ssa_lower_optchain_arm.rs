//! `Expr::OptChain { obj, name }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-70
//! of the decomp (chunks 1-69 = ... + `Expr::Ternary`).
//!
//! P3.5 — `obj?.field` returns `Type::Any`. Miss path emits
//! `ANY_UNDEF=5` box per ES spec §13.3.9 (short-circuit value is
//! `undefined`, not `null`); hit path loads the field and boxes as
//! Any. Result is a single Any-box ptr that downstream Any-aware
//! ops (typeof / strict-eq / nullish / print) handle uniformly.
//!
//! Pre-P3.5 the result was field-typed with `ConstPtrNull`/`0`
//! sentinel on miss — silently wrong (typeof returned the field's
//! typed-tier label, not `"undefined"`; print emitted `"null"`
//! instead of `"undefined"`).
//!
//! Two receiver shapes:
//!
//! - **`Type::Any`** — delegates to
//!   `LowerCtx::lower_optchain_any` (NaN-box tag-discriminated
//!   nullish check + Any.Member dispatch lives in
//!   `ssa_lower_optchain.rs`).
//! - **`Type::Obj(sid)` typed-tier pointer** — null-check via
//!   `ICmp::Eq(obj, ConstPtrNull)`, CondBr into null-block (boxes
//!   ANY_UNDEF) vs mem-block (load field + box_to_any). For non-
//!   pointer obj_ty (e.g. statically-known non-null Obj) the cond
//!   is constant-false and LLVM folds the null branch away.
//!
//! Non-Obj non-Any receivers panic (not yet supported).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::OptChain` match arm bottoms out here).

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, obj: crate::ast::ExprId, name: &str) -> Operand {
    let obj_op = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_op);
    if matches!(obj_ty, Type::Any) {
        return ctx.lower_optchain_any(obj_op, name);
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        _ => {
            panic!("ssa-lower: optional chain on non-struct obj type {obj_ty:?} not yet supported")
        }
    };
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optchain"));
    let cur_block = ctx.cur_block;
    let cond = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, obj_op.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let mem_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: null_blk,
            else_blk: mem_blk,
        },
    );
    emit_miss_path(ctx, null_blk, res_slot, after);
    emit_hit_path(ctx, mem_blk, obj_op, sid, name, res_slot, after);
    ctx.cur_block = after;
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

fn emit_miss_path(
    ctx: &mut LowerCtx<'_>,
    null_blk: crate::ssa::BlockId,
    res_slot: crate::ssa::ValueId,
    after: crate::ssa::BlockId,
) {
    ctx.cur_block = null_blk;
    let cur_block = ctx.cur_block;
    let undef_box = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        null_blk,
        InstKind::Store(Operand::Value(undef_box), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(null_blk, Terminator::Br(after));
}

fn emit_hit_path(
    ctx: &mut LowerCtx<'_>,
    mem_blk: crate::ssa::BlockId,
    obj_op: Operand,
    sid: crate::ssa::StructId,
    name: &str,
    res_slot: crate::ssa::ValueId,
    after: crate::ssa::BlockId,
) {
    ctx.cur_block = mem_blk;
    let (field_idx, field_ty) = {
        let layout = &ctx.struct_layouts[sid.0 as usize];
        layout
            .iter()
            .enumerate()
            .find(|(_, (n, _))| n == name)
            .map(|(i, (_, t))| (i, *t))
            .unwrap_or_else(|| panic!("ssa-lower: no field `{name}` on struct {sid:?}"))
    };
    let offset = OBJ_HEADER_SIZE + (field_idx as u64) * 8;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(field_ty, obj_op, offset),
        field_ty,
        None,
    );
    let boxed = ctx.box_to_any(Operand::Value(v));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(boxed, Operand::Value(res_slot), 0),
    );
    let mb = ctx.cur_block;
    ctx.f.set_term(mb, Terminator::Br(after));
}
