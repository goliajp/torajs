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
//! - **`Type::Obj(sid)` typed-tier pointer** — nullish-check (see
//!   below), CondBr into null-block (boxes ANY_UNDEF) vs mem-block
//!   (load field + box_to_any). For non-pointer obj_ty (e.g.
//!   statically-known non-null Obj) the cond is constant-false and
//!   LLVM folds the null branch away.
//! - **other sentinel-capable pointer receivers** (chunk 791 —
//!   Str / Substr / Arr / Closure / FnSig) — same branch scaffold;
//!   the hit path dispatches through the regular typed-receiver
//!   member ladder (`ssa_lower_member::lower_with_val`) so every
//!   member the plain `.` read supports works under `?.` too.
//!
//! The nullish check mirrors the chunk-786 `??` station: a
//! pointer-shaped slot's undefined is the immortal per-type
//! sentinel cell (RFC 20260710), not NULL — comparing only NULL
//! sent a sentinel-filled optional field down the hit path
//! (`o.child?.x` loaded garbage past the sentinel header). Nullish
//! = NULL or the receiver type's sentinel address.
//!
//! Receivers with no sentinel mapping panic (not yet supported).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::OptChain` match arm bottoms out here).

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    obj: crate::ast::ExprId,
    name: &str,
) -> Operand {
    let obj_op = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_op);
    if matches!(obj_ty, Type::Any) {
        return ctx.lower_optchain_any(eid, obj_op, name);
    }
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optchain"));
    let cur_block = ctx.cur_block;
    let eq_null = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, obj_op.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    // Chunk 791 — nullish = NULL or the receiver type's undefined
    // sentinel address (chunk-786 `??` station mirror): a filled
    // optional field holds the immortal sentinel cell, and the
    // NULL-only check rode it down the hit path (garbage field
    // load past the sentinel header).
    let Some(sentinel) = ctx.str_undef_sentinel_for(obj_ty) else {
        panic!("ssa-lower: optional chain on receiver type {obj_ty:?} not yet supported")
    };
    let eq_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, obj_op.clone(), sentinel),
        Type::Bool,
        None,
    );
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(eq_null),
            Operand::Value(eq_undef),
        ),
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
    if let Type::Obj(sid) = obj_ty {
        emit_hit_path(ctx, mem_blk, eid, obj_op, sid, name, res_slot, after);
    } else {
        emit_hit_path_member(ctx, mem_blk, eid, obj, obj_op, name, res_slot, after);
    }
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

#[allow(clippy::too_many_arguments)]
fn emit_hit_path(
    ctx: &mut LowerCtx<'_>,
    mem_blk: crate::ssa::BlockId,
    eid: crate::ast::ExprId,
    obj_op: Operand,
    sid: crate::ssa::StructId,
    name: &str,
    res_slot: crate::ssa::ValueId,
    after: crate::ssa::BlockId,
) {
    ctx.cur_block = mem_blk;
    let found = {
        let layout = &ctx.struct_layouts[sid.0 as usize];
        layout
            .iter()
            .enumerate()
            .find(|(_, (n, _))| n == name)
            .map(|(i, (_, t))| (i, *t))
    };
    // A name the LAYOUT does not carry is not a miss — the class keeps
    // its methods on the prototype, and an ordinary object is
    // extensible. `a.m` and `a.nosuch` have always answered that
    // through the any-member probe; the chain spelling now reaches the
    // same place instead of stopping the compile at a struct that was
    // never going to hold the answer.
    let Some((field_idx, field_ty)) = found else {
        let boxed_recv = ctx.box_to_any(obj_op);
        let v = crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed_recv, name);
        let blk = ctx.cur_block;
        ctx.f
            .append_void(blk, InstKind::Store(v, Operand::Value(res_slot), 0));
        ctx.f.set_term(blk, Terminator::Br(after));
        return;
    };
    let offset = OBJ_HEADER_SIZE + (field_idx as u64) * 8;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(field_ty, obj_op, offset),
        field_ty,
        None,
    );
    // Rotation 185 stake audit — the Load is a BORROW off the struct
    // slot and every box_to_any arm is a pure encode, yet the
    // OptChain result is consumed as an OWNED Any (let-decl slot
    // drop / discard release via owned_member_reads below): without
    // a compensating inc the release stole the slot's stake (the
    // Any-receiver twin `lower_any_member_read` incs + records; this
    // typed path did neither). Inc no-ops on NULL / the sentinels.
    if field_ty.is_refcounted() && !matches!(field_ty, Type::Any) {
        ctx.emit_rc_inc(Operand::Value(v));
        ctx.owned_member_reads.insert(eid);
    }
    let boxed = ctx.box_to_any(Operand::Value(v));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(boxed, Operand::Value(res_slot), 0),
    );
    let mb = ctx.cur_block;
    ctx.f.set_term(mb, Terminator::Br(after));
}

/// Chunk 791 — hit path for non-Obj sentinel-capable receivers
/// (`o.tag?.length` on an optional string field): dispatch the
/// member read through the regular typed-receiver ladder so every
/// member the plain `.` read supports works under `?.` too. Lanes
/// that already answer Any store the box as-is; typed results box
/// via `box_to_any` (Str-slot decode included).
#[allow(clippy::too_many_arguments)]
fn emit_hit_path_member(
    ctx: &mut LowerCtx<'_>,
    mem_blk: crate::ssa::BlockId,
    eid: crate::ast::ExprId,
    obj: crate::ast::ExprId,
    obj_op: Operand,
    name: &str,
    res_slot: crate::ssa::ValueId,
    after: crate::ssa::BlockId,
) {
    ctx.cur_block = mem_blk;
    let obj_ty = ctx.operand_ty(&obj_op);
    let v = crate::ssa_lower_member::lower_with_val(ctx, eid, obj, obj_op, obj_ty, name);
    let boxed = if matches!(ctx.operand_ty(&v), Type::Any) {
        v
    } else {
        ctx.box_to_any(v)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(boxed, Operand::Value(res_slot), 0),
    );
    let mb = ctx.cur_block;
    ctx.f.set_term(mb, Terminator::Br(after));
}
