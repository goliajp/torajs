//! `await` of an object typed as itself (`check::awaited`: its type
//! has no `then` and the program is proven to give no object one). Only
//! a promise cell is a thenable here, one under a type that lies (the
//! proof refuses an `as` of an `any`, but an inferred slot that took an
//! `any` is not cast), so the cell's tag picks the lane at run time.

use super::{await_any_in, await_cell, await_direct, suspend_direct};
use crate::ast::ExprId;
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// An object other than a promise held as its own heap cell, with the
/// universal header at offset 0: its tag tells a promise cell apart.
pub(crate) fn is_object_cell(ty: Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Map
            | Type::Set
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::MapIter
            | Type::ArrIter
    )
}

/// Await the object `op` of type `ty`. A promise cell is awaited and
/// its value read in the object's lane, anything else is kept across a
/// direct suspension; both answer it owned. A function pointer is no
/// cell (and no promise); an inline struct is boxed into a cell of its
/// own first and read back as one.
pub(super) fn await_object(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    op: Operand,
    ty: Type,
) -> Operand {
    if let Type::FnSig(_) = ty {
        return await_direct(ctx, eid, obj, op, ty);
    }
    debug_assert!(
        crate::builtin_proto_shadow::then_opening(ctx.ast, Some(ctx.expr_types)).is_none(),
        "an await typed as its object needs a closed `then` proof"
    );
    let v = if is_object_cell(ty) {
        if !ctx.expr_transfers_ownership(obj) {
            ctx.emit_owned_result_inc(op, ty);
        }
        await_cell_or_direct(ctx, eid, op, ty)
    } else {
        let lane = match ty {
            Type::Inline(sid) => Type::Obj(sid),
            t => t,
        };
        let borrowed = !ctx.expr_transfers_ownership(obj);
        let boxed = ctx.box_owned_from_expr(obj, op, borrowed);
        await_any_in(ctx, boxed, lane, false)
    };
    ctx.owned_member_reads.insert(eid);
    v
}

/// The owned cell `op` awaited when it is a promise, else kept across
/// a direct suspension; the two meet with its value in a slot.
fn await_cell_or_direct(ctx: &mut LowerCtx<'_>, eid: ExprId, op: Operand, ty: Type) -> Operand {
    let slot = ctx.alloca_in_entry(ty, Some("__awaited"));
    let (cell_blk, direct_blk) = branch_on_promise(ctx, op);
    let join = ctx.f.add_block();
    for (blk, cell) in [(cell_blk, true), (direct_blk, false)] {
        ctx.cur_block = blk;
        let v = if cell {
            let settled = await_cell(ctx, op, None, Some(Type::Any), None, false);
            cell_of(ctx, settled, ty)
        } else {
            suspend_direct(ctx, eid, op, ty)
        };
        let end = ctx.cur_block;
        ctx.f
            .append_void(end, InstKind::Store(v, Operand::Value(slot), 0));
        ctx.f.set_term(end, Terminator::Br(join));
    }
    ctx.cur_block = join;
    let load = InstKind::Load(ty, Operand::Value(slot), 0);
    Operand::Value(ctx.f.append_inst(join, load, ty, None))
}

/// The owned `any` value `v` a promise under the object type `ty`
/// settled with, as that type: its cell, or null when it holds none.
/// The type lied about the promise and may lie about its value too,
/// so the value is read in the `any` lane rather than in `ty`'s.
pub(crate) fn cell_of(ctx: &mut LowerCtx<'_>, v: Operand, ty: Type) -> Operand {
    let call = InstKind::Call(ctx.intrinsics.any_cell_ptr, vec![v]);
    let raw = ctx.f.append_inst(ctx.cur_block, call, Type::I64, None);
    let cast = InstKind::IntToPtr(Operand::Value(raw));
    Operand::Value(ctx.f.append_inst(ctx.cur_block, cast, ty, None))
}

/// End the current block on whether the pointer `op` is a promise
/// cell; answers the blocks for a cell and for anything else. A null
/// pointer is no cell: a cast decodes an `any` that holds no cell
/// (`null`, a number) to one, and `await null` fulfills.
fn branch_on_promise(ctx: &mut LowerCtx<'_>, op: Operand) -> (BlockId, BlockId) {
    let (tag_blk, cell_blk, direct_blk) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    let cb = ctx.cur_block;
    let null = InstKind::ICmp(IPred::Eq, op, Operand::ConstPtrNull);
    let null = Operand::Value(ctx.f.append_inst(cb, null, Type::Bool, None));
    ctx.f.set_term(cb, cond_br(null, direct_blk, tag_blk));
    // a 32-bit load at the tag takes the flags above it along
    let at = std::mem::offset_of!(torajs_rc::HeapHeader, type_tag) as u64;
    let word = ctx
        .f
        .append_inst(tag_blk, InstKind::Load(Type::I32, op, at), Type::I32, None);
    let tag = InstKind::BinOp(BinOp::And, Operand::Value(word), Operand::ConstI32(0xFFFF));
    let tag = ctx.f.append_inst(tag_blk, tag, Type::I32, None);
    let promise = Operand::ConstI32(torajs_rc::Tag::Promise as i32);
    let is_cell = InstKind::ICmp(IPred::Eq, Operand::Value(tag), promise);
    let is_cell = Operand::Value(ctx.f.append_inst(tag_blk, is_cell, Type::Bool, None));
    ctx.f
        .set_term(tag_blk, cond_br(is_cell, cell_blk, direct_blk));
    (cell_blk, direct_blk)
}

fn cond_br(cond: Operand, then_blk: BlockId, else_blk: BlockId) -> Terminator {
    Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    }
}
