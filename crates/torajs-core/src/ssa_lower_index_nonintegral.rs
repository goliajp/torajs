//! A Number index that is not an integer, on the TYPED array lanes —
//! `xs[1.5]` names the property "1.5", never element 1 (§7.1.19).
//! The read carries a "not an integer" flag next to the truncated
//! index and, when it fires, reads the expando the keyed kernel holds
//! (or the slot's undefined spelling); the store's arm lives in
//! `ssa_lower_index_assign_guards`. Sibling of
//! [`crate::ssa_lower_index_any_key`], whose `AnyNumericKey` is the
//! same split for an `any` receiver.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_index_any_key::AnyNumericKey;

/// The typed read's index, plus — for an index the SSA types F64 —
/// the "not an integer" flag and the raw f64. §7.1.19: `xs[1.5]`
/// names the property "1.5", never element 1, and `xs[NaN]` names
/// "NaN"; a typed array holds neither, so the read answers
/// `undefined` through the same exit an out-of-range index takes.
/// Before this every F64 index was truncated (`xs[1.5]` read
/// `xs[1]`, `xs[-0.5]` read `xs[0]`, `xs[NaN]` read `xs[0]`). The
/// truncation stays for the integral case — the flag is one
/// `fcvtzs` / `scvtf` / `fcmp` on top of it, on F64 indices only; an
/// I64 counter pays nothing.
pub(crate) fn lower_typed_read_index(
    ctx: &mut LowerCtx<'_>,
    index: ExprId,
) -> (Operand, Option<(Operand, Operand)>) {
    let raw = match &ctx.pins.compound_key {
        Some((mid, mop)) if *mid == index => mop.clone(),
        _ => ctx.lower_expr(index),
    };
    if !matches!(ctx.operand_ty(&raw), Type::F64) {
        return (ctx.coerce_to_i64(raw), None);
    }
    let idx = ctx.coerce_to_i64(raw.clone());
    let back = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::SiToFp(idx.clone()),
        Type::F64,
        None,
    );
    let nonint = ctx.fcmp(crate::ssa::FPred::Une, Operand::Value(back), raw.clone());
    (idx, Some((nonint, raw)))
}

/// The typed read's arm for an index that is not an integer: the
/// property "1.5" may exist as an expando (`xs[1.5] = v` makes one),
/// so the keyed kernel is asked on the boxed array and the answer is
/// unboxed into the element's slot shape — undefined to the slot's
/// sentinel, a number to f64, a Str cell to the Str slot; any other
/// value in a typed slot is the lane's recorded approximation and
/// reads as the sentinel. Answers `false` for an element type with no
/// undefined spelling (i64 / bool / nested heap): the caller keeps
/// its RangeError exit. Leaves `cur_block` on the block that stored
/// the slot, with no terminator.
pub(crate) fn emit_nonintegral_typed_read(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    raw_key: Operand,
    elem_ty: Type,
    slot: ValueId,
) -> bool {
    use crate::ssa::{IPred, Terminator};
    if !matches!(elem_ty, Type::F64 | Type::Str) {
        return false;
    }
    let boxed_arr = ctx.box_to_any(arr);
    let boxed_key = ctx.box_to_any(raw_key);
    let v = AnyNumericKey::Boxed(boxed_key).emit_get(ctx, boxed_arr);
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![Operand::Value(v)]),
        Type::I64,
        None,
    );
    let undef_blk = ctx.f.add_block();
    let val_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    let is_undef = ctx.cmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5));
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: is_undef,
            then_blk: undef_blk,
            else_blk: val_blk,
        },
    );
    // undefined (and, for a Str slot, any non-Str answer) → sentinel.
    ctx.cur_block = undef_blk;
    let sentinel = match elem_ty {
        Type::F64 => Operand::ConstF64(f64::from_bits(
            crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
        )),
        _ => {
            let c = ctx.f.append_inst(
                undef_blk,
                InstKind::GlobalRef(
                    crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM.to_string(),
                ),
                Type::Str,
                None,
            );
            Operand::Value(c)
        }
    };
    ctx.f.append_void(
        undef_blk,
        InstKind::Store(sentinel, Operand::Value(slot), 0),
    );
    ctx.emit_drop_value(Operand::Value(v), Type::Any);
    let ub = ctx.cur_block;
    ctx.f.set_term(ub, Terminator::Br(done_blk));
    ctx.cur_block = val_blk;
    match elem_ty {
        Type::F64 => {
            let f = ctx.f.append_inst(
                val_blk,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![Operand::Value(v)]),
                Type::F64,
                None,
            );
            ctx.f.append_void(
                val_blk,
                InstKind::Store(Operand::Value(f), Operand::Value(slot), 0),
            );
            ctx.emit_drop_value(Operand::Value(v), Type::Any);
        }
        _ => {
            // A Str cell answers +1 through the owned unbox; anything
            // else reads as the sentinel (approximation, above).
            let is_cell = ctx.cmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(4));
            let cell_blk = ctx.f.add_block();
            let other_blk = ctx.f.add_block();
            let vb = ctx.cur_block;
            ctx.f.set_term(
                vb,
                Terminator::CondBr {
                    cond: is_cell,
                    then_blk: cell_blk,
                    else_blk: other_blk,
                },
            );
            let ptr = ctx.f.append_inst(
                cell_blk,
                InstKind::Call(
                    ctx.intrinsics.any_unbox_value_owned,
                    vec![Operand::Value(v)],
                ),
                Type::Str,
                None,
            );
            ctx.f.append_void(
                cell_blk,
                InstKind::Store(Operand::Value(ptr), Operand::Value(slot), 0),
            );
            ctx.cur_block = cell_blk;
            ctx.emit_drop_value(Operand::Value(v), Type::Any);
            let cbk = ctx.cur_block;
            ctx.f.set_term(cbk, Terminator::Br(done_blk));
            ctx.cur_block = other_blk;
            let c = ctx.f.append_inst(
                other_blk,
                InstKind::GlobalRef(
                    crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM.to_string(),
                ),
                Type::Str,
                None,
            );
            ctx.f.append_void(
                other_blk,
                InstKind::Store(Operand::Value(c), Operand::Value(slot), 0),
            );
            ctx.emit_drop_value(Operand::Value(v), Type::Any);
        }
    }
    let lb = ctx.cur_block;
    ctx.f.set_term(lb, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    true
}
