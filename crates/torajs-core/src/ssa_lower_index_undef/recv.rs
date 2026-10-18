//! `xs[i].f` on a class-typed element: the element read lowered as
//! the receiver of a field read.
//!
//! The plain read (`lower_typed_index_checked`) answers `undefined`
//! for an out-of-range index or a hole, merges that with the loaded
//! element through a slot, and the field read then tests the merged
//! value again before dereferencing it. Here the consumer is known:
//! a field read on `undefined` is a TypeError (§13.3.2 GetValue on an
//! undefined base), so every exit that would have produced
//! `undefined` raises it directly, with the same kernel and wording
//! the receiver guard uses, and the live element flows on without a
//! merge. What remains on the path is one bounds compare, one load
//! and one nullish test.
//!
//! The test on the loaded word covers both spellings a typed slot can
//! hold for "no element": NULL (a hole, or the runtime's zero fill)
//! and the generic undefined cell (a miss stored back into the
//! array). Either answers `undefined` on the plain read, so both take
//! the same raise. An element held in place (`Type::Inline`) is its
//! address, and its array has no word that spells a missing element,
//! so the bounds compare is its only exit.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;

pub(crate) fn lower_typed_index_receiver(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
    is_non_deque: bool,
    elem_ty: Type,
    upper_proven: bool,
) -> Operand {
    let raise_blk = ctx.f.add_block();
    let in_blk = ctx.f.add_block();
    let tests = match elem_ty {
        Type::Inline(_) => None,
        _ => Some((ctx.f.add_block(), ctx.f.add_block())),
    };
    // one unsigned compare covers both ends; a proven upper bound
    // leaves only the sign test (`lower_typed_index_checked`)
    let out = if upper_proven {
        ctx.cmp(IPred::Slt, idx_val, Operand::ConstI64(0))
    } else {
        let cb = ctx.cur_block;
        let len = ctx.f.append_inst(
            cb,
            InstKind::Load(Type::I64, arr_val, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        ctx.cmp(IPred::Uge, idx_val, Operand::Value(len))
    };
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: out,
            then_blk: raise_blk,
            else_blk: in_blk,
        },
    );
    ctx.cur_block = in_blk;
    let (v, live) = match (elem_ty, tests) {
        (Type::Inline(sid), _) => {
            let v =
                crate::ssa_lower::inline_elem::elem_addr(ctx, arr_val, idx_val, sid, is_non_deque);
            (v, ctx.cur_block)
        }
        (_, Some((cell_test, live))) => {
            let base = (arr_val, idx_val, is_non_deque);
            let v = load_and_test(ctx, base, elem_ty, raise_blk, cell_test, live);
            (v, live)
        }
        (_, None) => unreachable!("a cell element read always has its tests"),
    };
    // every exit here is the plain read's `undefined`, whichever
    // word the slot held, so the raise names undefined
    ctx.cur_block = raise_blk;
    let undef = ctx.f.append_inst(
        raise_blk,
        InstKind::GlobalRef(UNDEF_CELL_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let raiser = ctx.intrinsics.heap_nullish_check;
    crate::ssa_lower_nullable_guard_emit::emit_nullish_raise(ctx, raiser, Operand::Value(undef));
    ctx.cur_block = live;
    Operand::Value(v)
}

/// The slot word of element `idx` of a cell array, with NULL sent to
/// `raise_blk` and the undefined cell sent there from `cell_test`; the
/// word flows on into `live`.
fn load_and_test(
    ctx: &mut LowerCtx<'_>,
    (arr_val, idx_val, is_non_deque): (Operand, Operand, bool),
    elem_ty: Type,
    raise_blk: BlockId,
    cell_test: BlockId,
    live: BlockId,
) -> ValueId {
    let (base, off) = ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 8, is_non_deque);
    let cb = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cb, InstKind::LoadDyn(elem_ty, base, off), elem_ty, None);
    let is_null = ctx.cmp(IPred::Eq, Operand::Value(v), Operand::ConstPtrNull);
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: is_null,
            then_blk: raise_blk,
            else_blk: cell_test,
        },
    );
    ctx.cur_block = cell_test;
    let cell = ctx.f.append_inst(
        cell_test,
        InstKind::GlobalRef(UNDEF_CELL_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let is_cell = ctx.cmp(IPred::Eq, Operand::Value(v), Operand::Value(cell));
    ctx.f.set_term(
        cell_test,
        Terminator::CondBr {
            cond: is_cell,
            then_blk: raise_blk,
            else_blk: live,
        },
    );
    v
}
