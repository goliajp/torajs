//! Inline fast arms for `any` values: NaN-box tests and header reads
//! emitted as SSA, with the runtime helper kept as the slow arm for
//! every encoding the fast arm does not cover.

use torajs_anyvalue::nanbox::{TAG_BIT_TYPE_OTHER, TAG_TYPE_NUMBER, TOP_16_MASK};

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// `recv.length` for an `any` receiver. A plain or substring Str cell
/// answers the u32 length at +8 (an inline Substr keeps a u64 there,
/// never past its parent's u32) boxed as an int32, which is what
/// `__torajs_any_length_get` answers for it; every other encoding —
/// ShortStr, Proxy, wrappers, arrays, null / undefined (TypeError) —
/// and a length past i32 (boxed as a double) take the helper call.
pub(crate) fn lower_any_length(ctx: &mut LowerCtx<'_>, recv: Operand) -> Operand {
    let slot = ctx.alloca(Type::Any, None);
    let hdr_blk = ctx.f.add_block();
    let fast_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();

    let bits = emit_bits(ctx, &recv);
    let is_cell = emit_is_cell(ctx, bits);
    branch(ctx, is_cell, hdr_blk, slow_blk);

    // bytes 4..12 of the cell: type_tag u16 | flags u16 | length low 32
    ctx.cur_block = hdr_blk;
    let p = inst(ctx, InstKind::IntToPtr(Operand::Value(bits)), Type::Ptr);
    let word = inst(
        ctx,
        InstKind::Load(Type::I64, Operand::Value(p), 4),
        Type::I64,
    );
    // one test: the tag is Str and the length fits an int32
    let tag_and_len_sign = 0x8000_0000_0000_FFFFu64 as i64;
    let probe = inst(
        ctx,
        bin(BinOp::And, word, Operand::ConstI64(tag_and_len_sign)),
        Type::I64,
    );
    let is_str = inst(
        ctx,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(probe),
            Operand::ConstI64(torajs_rc::Tag::Str as i64),
        ),
        Type::Bool,
    );
    branch(ctx, is_str, fast_blk, slow_blk);

    ctx.cur_block = fast_blk;
    let len = inst(
        ctx,
        bin(BinOp::LShr, word, Operand::ConstI64(32)),
        Type::I64,
    );
    let boxed = inst(
        ctx,
        bin(BinOp::Or, len, Operand::ConstI64(TAG_TYPE_NUMBER as i64)),
        Type::I64,
    );
    let v = inst(ctx, InstKind::IntToPtr(Operand::Value(boxed)), Type::Any);
    store_and_join(ctx, v, slot, done_blk);

    ctx.cur_block = slow_blk;
    let v = inst(
        ctx,
        InstKind::Call(ctx.intrinsics.any_length_get, vec![recv]),
        Type::Any,
    );
    ctx.emit_throw_check(None);
    store_and_join(ctx, v, slot, done_blk);

    ctx.cur_block = done_blk;
    Operand::Value(inst(
        ctx,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
    ))
}

fn emit_bits(ctx: &mut LowerCtx<'_>, v: &Operand) -> ValueId {
    inst(ctx, InstKind::PtrToInt(v.clone()), Type::I64)
}

/// `bits` is a heap cell: non-zero with the top 16 bits and
/// TAG_BIT_TYPE_OTHER clear (every number, ShortStr and sentinel
/// encoding sets one of them).
fn emit_is_cell(ctx: &mut LowerCtx<'_>, bits: ValueId) -> ValueId {
    let mask = (TOP_16_MASK | TAG_BIT_TYPE_OTHER) as i64;
    let tagged = inst(
        ctx,
        bin(BinOp::And, bits, Operand::ConstI64(mask)),
        Type::I64,
    );
    let untagged = inst(ctx, cmp_zero(IPred::Eq, tagged), Type::Bool);
    let nonzero = inst(ctx, cmp_zero(IPred::Ne, bits), Type::Bool);
    inst(
        ctx,
        InstKind::BinOp(
            BinOp::And,
            Operand::Value(untagged),
            Operand::Value(nonzero),
        ),
        Type::Bool,
    )
}

fn inst(ctx: &mut LowerCtx<'_>, kind: InstKind, ty: Type) -> ValueId {
    ctx.f.append_inst(ctx.cur_block, kind, ty, None)
}

fn bin(op: BinOp, a: ValueId, b: Operand) -> InstKind {
    InstKind::BinOp(op, Operand::Value(a), b)
}

fn cmp_zero(pred: IPred, v: ValueId) -> InstKind {
    InstKind::ICmp(pred, Operand::Value(v), Operand::ConstI64(0))
}

fn branch(ctx: &mut LowerCtx<'_>, cond: ValueId, then_blk: BlockId, else_blk: BlockId) {
    let b = ctx.cur_block;
    ctx.f.set_term(
        b,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk,
            else_blk,
        },
    );
}

fn store_and_join(ctx: &mut LowerCtx<'_>, v: ValueId, slot: ValueId, done: BlockId) {
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    let b = ctx.cur_block;
    ctx.f.set_term(b, Terminator::Br(done));
}
