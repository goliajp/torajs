//! Inline fast arms for `any` values: NaN-box tests and header reads
//! emitted as SSA, with the runtime helper kept as the slow arm for
//! every encoding the fast arm does not cover.

use torajs_anyvalue::nanbox::{
    DOUBLE_ENCODE_OFFSET, TAG_BIT_TYPE_OTHER, TAG_TYPE_NUMBER, TOP_16_MASK,
};

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
    store_and_join(ctx, Operand::Value(v), slot, done_blk);

    ctx.cur_block = slow_blk;
    let v = inst(
        ctx,
        InstKind::Call(ctx.intrinsics.any_length_get, vec![recv]),
        Type::Any,
    );
    ctx.emit_throw_check(None);
    store_and_join(ctx, Operand::Value(v), slot, done_blk);

    ctx.cur_block = done_blk;
    Operand::Value(inst(
        ctx,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
    ))
}

/// §7.1.4 ToNumber of an `any` value into an F64. An int32 or double
/// encoding decodes inline; everything else calls
/// `__torajs_anyv_to_number` and checks for a pending throw.
pub(crate) fn to_number(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let slot = ctx.alloca(Type::F64, None);
    let num_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();

    let bits = emit_bits(ctx, &v);
    let is_num = emit_is_number(ctx, bits);
    branch(ctx, is_num, num_blk, slow_blk);

    ctx.cur_block = num_blk;
    emit_decode_number(ctx, bits, slot, done_blk);

    ctx.cur_block = slow_blk;
    let n = inst(
        ctx,
        InstKind::Call(ctx.intrinsics.any_to_number, vec![v]),
        Type::F64,
    );
    ctx.emit_throw_check(None);
    store_and_join(ctx, Operand::Value(n), slot, done_blk);

    ctx.cur_block = done_blk;
    Operand::Value(inst(
        ctx,
        InstKind::Load(Type::F64, Operand::Value(slot), 0),
        Type::F64,
    ))
}

/// `f + v` (or `v + f` when `!f_on_left`) for an F64 `f` and an `any`
/// `v`. When `v` encodes a number, §13.15.3 reduces to Number::add:
/// the sum is added in source order and boxed exactly as
/// `__torajs_anyv_add_pair` boxes a sum with a non-integer operand
/// (`box_double`, NaN collisions canonicalized). Any other `v` takes
/// `slow`, which must answer an owned `any`.
pub(crate) fn f64_add_any(
    ctx: &mut LowerCtx<'_>,
    f: Operand,
    v: &Operand,
    f_on_left: bool,
    slow: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let res = ctx.alloca(Type::Any, None);
    let num = ctx.alloca(Type::F64, None);
    let num_blk = ctx.f.add_block();
    let add_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();

    let bits = emit_bits(ctx, v);
    let is_num = emit_is_number(ctx, bits);
    branch(ctx, is_num, num_blk, slow_blk);

    ctx.cur_block = num_blk;
    emit_decode_number(ctx, bits, num, add_blk);

    ctx.cur_block = add_blk;
    let x = Operand::Value(inst(
        ctx,
        InstKind::Load(Type::F64, Operand::Value(num), 0),
        Type::F64,
    ));
    let (l, r) = if f_on_left { (f, x) } else { (x, f) };
    let sum = inst(ctx, InstKind::BinOp(BinOp::FAdd, l, r), Type::F64);
    emit_box_double(ctx, sum, res, done_blk);

    ctx.cur_block = slow_blk;
    let r = slow(ctx);
    store_and_join(ctx, r, res, done_blk);

    ctx.cur_block = done_blk;
    Operand::Value(inst(
        ctx,
        InstKind::Load(Type::Any, Operand::Value(res), 0),
        Type::Any,
    ))
}

/// `bits` encodes a number (int32 or double): `bits & TAG_TYPE_NUMBER
/// != 0`. Cells, ShortStr and the sentinels all have it clear.
fn emit_is_number(ctx: &mut LowerCtx<'_>, bits: ValueId) -> ValueId {
    let t = inst(
        ctx,
        bin(BinOp::And, bits, Operand::ConstI64(TAG_TYPE_NUMBER as i64)),
        Type::I64,
    );
    inst(ctx, cmp_zero(IPred::Ne, t), Type::Bool)
}

/// Decode a known-number encoding into `slot` and branch to `done`:
/// an int32 (top 16 bits == TAG_TYPE_NUMBER) sign-extends its low 32
/// bits, a double subtracts DOUBLE_ENCODE_OFFSET.
fn emit_decode_number(ctx: &mut LowerCtx<'_>, bits: ValueId, slot: ValueId, done: BlockId) {
    let int_blk = ctx.f.add_block();
    let dbl_blk = ctx.f.add_block();
    let top = inst(
        ctx,
        bin(BinOp::LShr, bits, Operand::ConstI64(48)),
        Type::I64,
    );
    let is_int = inst(
        ctx,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(top),
            Operand::ConstI64((TAG_TYPE_NUMBER >> 48) as i64),
        ),
        Type::Bool,
    );
    branch(ctx, is_int, int_blk, dbl_blk);

    ctx.cur_block = int_blk;
    let hi = inst(ctx, bin(BinOp::Shl, bits, Operand::ConstI64(32)), Type::I64);
    let n = inst(ctx, bin(BinOp::AShr, hi, Operand::ConstI64(32)), Type::I64);
    let f = inst(ctx, InstKind::SiToFp(Operand::Value(n)), Type::F64);
    store_and_join(ctx, Operand::Value(f), slot, done);

    ctx.cur_block = dbl_blk;
    let raw = inst(
        ctx,
        bin(
            BinOp::Sub,
            bits,
            Operand::ConstI64(DOUBLE_ENCODE_OFFSET as i64),
        ),
        Type::I64,
    );
    let f = inst(
        ctx,
        InstKind::BitCastI64ToF64(Operand::Value(raw)),
        Type::F64,
    );
    store_and_join(ctx, Operand::Value(f), slot, done);
}

/// `box_double(f)` into `slot`, then branch to `done`: the encoded
/// bits must still read as a double, otherwise (a NaN payload that
/// wraps into another tag's range) the canonical NaN is stored.
fn emit_box_double(ctx: &mut LowerCtx<'_>, f: ValueId, slot: ValueId, done: BlockId) {
    let ok_blk = ctx.f.add_block();
    let nan_blk = ctx.f.add_block();
    let raw = inst(ctx, InstKind::BitCastF64ToI64(Operand::Value(f)), Type::I64);
    let enc = inst(
        ctx,
        bin(
            BinOp::Add,
            raw,
            Operand::ConstI64(DOUBLE_ENCODE_OFFSET as i64),
        ),
        Type::I64,
    );
    let num_bits = inst(
        ctx,
        bin(BinOp::And, enc, Operand::ConstI64(TAG_TYPE_NUMBER as i64)),
        Type::I64,
    );
    let top = inst(
        ctx,
        bin(BinOp::And, enc, Operand::ConstI64(TOP_16_MASK as i64)),
        Type::I64,
    );
    let some_num = inst(ctx, cmp_zero(IPred::Ne, num_bits), Type::Bool);
    let not_int = inst(
        ctx,
        InstKind::ICmp(
            IPred::Ne,
            Operand::Value(top),
            Operand::ConstI64(TAG_TYPE_NUMBER as i64),
        ),
        Type::Bool,
    );
    let is_double = inst(
        ctx,
        InstKind::BinOp(
            BinOp::And,
            Operand::Value(some_num),
            Operand::Value(not_int),
        ),
        Type::Bool,
    );
    branch(ctx, is_double, ok_blk, nan_blk);

    ctx.cur_block = ok_blk;
    let v = inst(ctx, InstKind::IntToPtr(Operand::Value(enc)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);

    ctx.cur_block = nan_blk;
    let nan = f64::NAN.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET) as i64;
    let v = inst(ctx, InstKind::IntToPtr(Operand::ConstI64(nan)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);
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

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, slot: ValueId, done: BlockId) {
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
    let b = ctx.cur_block;
    ctx.f.set_term(b, Terminator::Br(done));
}
