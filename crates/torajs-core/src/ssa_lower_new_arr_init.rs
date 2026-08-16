//! Map / Set `<Array<...>>` init walker helpers split off from
//! [`crate::ssa_lower_new`] to keep both files under the 500-line
//! file-size hard limit. The two arr-init walkers share an identical
//! header/body/after CFG plus i-slot bump pattern; only the per-
//! element extraction differs (Map reads inner `[0]` / `[1]` as
//! key/value; Set reads outer slot as single key).
//!
//! - **S156**: `lower_map_from_arr` — outer `Array<Array<T>>` walker
//!   for `new Map(<typed Array<Array<T>>>)`. Per outer slot Load
//!   inner Array<T>, then index 0/1 as key/value, `box_to_tag_value`
//!   encode, `map_set` write. Drops outer array post-loop (we own
//!   the src).
//! - **S152**: `lower_set_from_arr` — `Array<T>` walker for
//!   `new Set(<typed Array<T>>)`. Per slot Load elem,
//!   `box_to_tag_value`, `map_set` (set_op uses `map_set` since
//!   Set storage = Map storage). Drops src array post-loop.
//! - **`bump_loop_i`** — shared `i_slot += 1` emit used by both
//!   walkers' body→header back-edge.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

pub(crate) fn lower_map_from_arr(
    ctx: &mut LowerCtx<'_>,
    map_op: Operand,
    arg_op: Operand,
    arg_ty: Type,
    arg_owned: bool,
    inner_id: crate::ssa::ArrId,
) -> Operand {
    let outer_arr = match arg_op {
        Operand::Value(v) => v,
        _ => unreachable!("Arr is SSA Value"),
    };
    let inner_elem_ty = ctx.arr_layouts[inner_id.0 as usize];
    let cur_block = ctx.cur_block;
    let len = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(outer_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__map_init_i"));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let cur_block = ctx.cur_block;
    let i_now = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    let cur_block = ctx.cur_block;
    let i_body = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let (outer_off_base, outer_off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(outer_arr), Operand::Value(i_body), 3, false);
    let cur_block = ctx.cur_block;
    let inner_arr = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(Type::Arr(inner_id), outer_off_base.clone(), outer_off),
        Type::Arr(inner_id),
        None,
    );
    // An Array<Any> slot is one 8-byte NaN-boxed AnyValue, same
    // stride as every other array (`torajs-arr` any.rs). This site
    // used to scale Any slots by 16 — the legacy (tag, value) pair
    // layout — which is invisible at index 0 and put the value read
    // of `new Map(pairs)` one slot past the pair, so every mixed-type
    // entry landed with a null value.
    let (k_base, k_off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(inner_arr), Operand::ConstI64(0), 3, false);
    let (v_base, v_off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(inner_arr), Operand::ConstI64(1), 3, false);
    let cur_block = ctx.cur_block;
    let k_loaded = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(inner_elem_ty, k_base, k_off),
        inner_elem_ty,
        None,
    );
    let v_loaded = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(inner_elem_ty, v_base, v_off),
        inner_elem_ty,
        None,
    );
    let (k_tag, k_val) = ctx.box_to_tag_value(Operand::Value(k_loaded));
    let (v_tag, v_val) = ctx.box_to_tag_value(Operand::Value(v_loaded));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.map_set,
            vec![map_op.clone(), k_tag, k_val, v_tag, v_val],
        ),
    );
    bump_loop_i(ctx, i_slot);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    // Rotation 325 — an owned source temp only; an ident-bound borrow
    // keeps its binding's stake (the unconditional drop double-freed
    // the pair arrays out from under the binding — census underflow
    // on check-map-init-arr-001).
    if arg_owned {
        ctx.emit_drop_value(Operand::Value(outer_arr), arg_ty);
    }
    map_op
}

pub(crate) fn lower_set_from_arr(
    ctx: &mut LowerCtx<'_>,
    set_op: Operand,
    arg_op: Operand,
    arg_ty: Type,
    arg_owned: bool,
    arr_id: crate::ssa::ArrId,
) -> Operand {
    let arr_val = match arg_op {
        Operand::Value(v) => v,
        _ => unreachable!("Arr is SSA Value"),
    };
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let cur_block = ctx.cur_block;
    let len = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_val), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__set_init_i"));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let cur_block = ctx.cur_block;
    let i_now = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    let cur_block = ctx.cur_block;
    let i_body = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let (off_base, off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_val), Operand::Value(i_body), 3, false);
    let cur_block = ctx.cur_block;
    let elem = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(elem_ty, off_base.clone(), off),
        elem_ty,
        None,
    );
    let (k_tag, k_val) = ctx.box_to_tag_value(Operand::Value(elem));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.map_set,
            vec![
                set_op.clone(),
                k_tag,
                k_val,
                Operand::ConstI64(5),
                Operand::ConstI64(0),
            ],
        ),
    );
    bump_loop_i(ctx, i_slot);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    // Rotation 325 — owned source temp only; see lower_map_from_arr.
    if arg_owned {
        ctx.emit_drop_value(Operand::Value(arr_val), arg_ty);
    }
    set_op
}

fn bump_loop_i(ctx: &mut LowerCtx<'_>, i_slot: crate::ssa::ValueId) {
    let cur_block = ctx.cur_block;
    let i_then = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
}
