//! The argument of a pointer slot that may hold NULL or the
//! `undefined` cell, serialized without leaving the static unfold.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;

/// The pointer types the static unfold serializes from the value
/// alone; the others (a closure, a symbol, a BigInt, a bare pointer)
/// answer at run time through the any-lane walk.
pub(super) fn unfolds_live_only(ty: Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Date
            | Type::RegExp
            | Type::Promise
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
    )
}

/// The argument of a pointer slot that may hold NULL or the
/// `undefined` cell: the cell answers the undefined value (§25.5.2
/// step 12), NULL the text `null`, and any other value is one of the
/// slot's static type, which `live` serializes.
pub(super) fn split_nullish(
    ctx: &mut LowerCtx<'_>,
    val_op: Operand,
    live: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let out_slot = ctx.alloca_in_entry(Type::Str, Some("__json_top_out"));
    let ty = ctx.operand_ty(&val_op);
    let cell = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        ty,
        None,
    );
    let is_cell = ctx.cmp(IPred::Eq, val_op, Operand::Value(cell));
    let (cell_blk, rest_blk) = (ctx.f.add_block(), ctx.f.add_block());
    let (null_blk, live_blk, done_blk) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: is_cell,
            then_blk: cell_blk,
            else_blk: rest_blk,
        },
    );
    ctx.cur_block = rest_blk;
    let is_null = ctx.cmp(IPred::Eq, val_op, Operand::ConstPtrNull);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: is_null,
            then_blk: null_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = cell_blk;
    let undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
        Type::Str,
        None,
    );
    store_and_join(ctx, Operand::Value(undef), out_slot, done_blk);
    ctx.cur_block = null_blk;
    let null = Operand::Value(ctx.intern_string_literal("null"));
    store_and_join(ctx, null, out_slot, done_blk);
    ctx.cur_block = live_blk;
    let v = live(ctx);
    store_and_join(ctx, v, out_slot, done_blk);
    ctx.cur_block = done_blk;
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(out)
}

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, slot: ValueId, join: BlockId) {
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
}
