//! Inline number arms for `any` values: ToNumber of an any and `+`
//! with an any addend, decoding the NaN-box in SSA and keeping the
//! runtime call for every encoding that is not a number.

use torajs_anyvalue::nanbox::{DOUBLE_ENCODE_OFFSET, TAG_TYPE_NUMBER, TOP_16_MASK};

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_anyv_inline::{bin, branch, cmp_zero, emit_bits, inst, store_and_join};

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
    emit_decode_number(ctx, bits, slot, None, done_blk);

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

/// `a + b` where at least one side is `any` and the other is `any`,
/// F64 or I64. When every `any` side encodes a number, §13.15.3
/// reduces to Number::add: the operands are added in source order and
/// the sum is boxed as `__torajs_anyv_add_pair` boxes it — an int32
/// when both operands are integer-shaped (an int32 encoding or an I64)
/// and the sum is an integer in int32 range, `box_double` otherwise
/// (NaN collisions canonicalized). Two int32 encodings add as
/// integers, which gives the same box without a float round trip.
/// Any other value takes `slow`, which must answer an owned `any`.
pub(crate) fn number_add(
    ctx: &mut LowerCtx<'_>,
    (a, a_ty): (Operand, Type),
    (b, b_ty): (Operand, Type),
    slow: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let res = ctx.alloca(Type::Any, None);
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    if a_ty == Type::Any && b_ty == Type::Any {
        let general = ctx.f.add_block();
        emit_int32_pair_add(ctx, (&a, &b), res, general, done_blk);
        ctx.cur_block = general;
    }
    let (l, l_int) = emit_number_side(ctx, &a, a_ty, slow_blk);
    let (r, r_int) = emit_number_side(ctx, &b, b_ty, slow_blk);
    let sum = inst(ctx, InstKind::BinOp(BinOp::FAdd, l, r), Type::F64);
    let both_int = match (l_int, r_int) {
        (IntShaped::No, _) | (_, IntShaped::No) => IntShaped::No,
        (IntShaped::Yes, x) | (x, IntShaped::Yes) => x,
        (IntShaped::If(x), IntShaped::If(y)) => IntShaped::If(inst(
            ctx,
            InstKind::BinOp(BinOp::And, Operand::Value(x), Operand::Value(y)),
            Type::Bool,
        )),
    };
    emit_box_sum(ctx, sum, both_int, res, done_blk);

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

/// Both `any` addends int32 encodings: add them as i64 (no overflow
/// there), box an int32 when the sum fits, else the double of it, and
/// branch to `done`; anything else branches to `general`.
fn emit_int32_pair_add(
    ctx: &mut LowerCtx<'_>,
    (a, b): (&Operand, &Operand),
    slot: ValueId,
    general: BlockId,
    done: BlockId,
) {
    let int_tag = Operand::ConstI64((TAG_TYPE_NUMBER >> 48) as i64);
    let int_of = |ctx: &mut LowerCtx<'_>, v: &Operand| {
        let bits = emit_bits(ctx, v);
        let top = inst(
            ctx,
            bin(BinOp::LShr, bits, Operand::ConstI64(48)),
            Type::I64,
        );
        let is_int = inst(
            ctx,
            InstKind::ICmp(IPred::Eq, Operand::Value(top), int_tag),
            Type::Bool,
        );
        (bits, is_int)
    };
    let (a_bits, a_int) = int_of(ctx, a);
    let (b_bits, b_int) = int_of(ctx, b);
    let both = inst(
        ctx,
        InstKind::BinOp(BinOp::And, Operand::Value(a_int), Operand::Value(b_int)),
        Type::Bool,
    );
    let add_blk = ctx.f.add_block();
    branch(ctx, both, add_blk, general);

    ctx.cur_block = add_blk;
    let fits_blk = ctx.f.add_block();
    let wide_blk = ctx.f.add_block();
    let (x, y) = (sext32(ctx, a_bits), sext32(ctx, b_bits));
    let sum = inst(
        ctx,
        InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::Value(y)),
        Type::I64,
    );
    let narrow = sext32(ctx, sum);
    let fits = inst(
        ctx,
        InstKind::ICmp(IPred::Eq, Operand::Value(narrow), Operand::Value(sum)),
        Type::Bool,
    );
    branch(ctx, fits, fits_blk, wide_blk);

    ctx.cur_block = fits_blk;
    let low = inst(
        ctx,
        bin(BinOp::And, sum, Operand::ConstI64(0xFFFF_FFFF)),
        Type::I64,
    );
    let boxed = inst(
        ctx,
        bin(BinOp::Or, low, Operand::ConstI64(TAG_TYPE_NUMBER as i64)),
        Type::I64,
    );
    let v = inst(ctx, InstKind::IntToPtr(Operand::Value(boxed)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);

    ctx.cur_block = wide_blk;
    let f = inst(ctx, InstKind::SiToFp(Operand::Value(sum)), Type::F64);
    emit_box_double(ctx, f, slot, done);
}

/// The low 32 bits of `v`, sign-extended.
fn sext32(ctx: &mut LowerCtx<'_>, v: ValueId) -> ValueId {
    let hi = inst(ctx, bin(BinOp::Shl, v, Operand::ConstI64(32)), Type::I64);
    inst(ctx, bin(BinOp::AShr, hi, Operand::ConstI64(32)), Type::I64)
}

/// Whether an addend is integer-shaped (`tag_is_i64_shaped` in the
/// kernel): known, or a Bool decided at run time.
#[derive(Clone, Copy)]
enum IntShaped {
    No,
    Yes,
    If(ValueId),
}

/// One addend as an F64 and its integer shape. An `any` side that
/// does not encode a number branches to `slow`.
fn emit_number_side(
    ctx: &mut LowerCtx<'_>,
    v: &Operand,
    ty: Type,
    slow: BlockId,
) -> (Operand, IntShaped) {
    match ty {
        Type::F64 => (*v, IntShaped::No),
        Type::I64 => {
            let f = inst(ctx, InstKind::SiToFp(*v), Type::F64);
            (Operand::Value(f), IntShaped::Yes)
        }
        _ => {
            let num = ctx.alloca(Type::F64, None);
            let int = ctx.alloca(Type::Bool, None);
            let num_blk = ctx.f.add_block();
            let next = ctx.f.add_block();
            let bits = emit_bits(ctx, v);
            let is_num = emit_is_number(ctx, bits);
            branch(ctx, is_num, num_blk, slow);
            ctx.cur_block = num_blk;
            emit_decode_number(ctx, bits, num, Some(int), next);
            ctx.cur_block = next;
            let f = inst(
                ctx,
                InstKind::Load(Type::F64, Operand::Value(num), 0),
                Type::F64,
            );
            let i = inst(
                ctx,
                InstKind::Load(Type::Bool, Operand::Value(int), 0),
                Type::Bool,
            );
            (Operand::Value(f), IntShaped::If(i))
        }
    }
}

/// Box a sum into `slot` and branch to `done`: an int32 when
/// `both_int` holds and the sum is an integer in int32 range (two
/// integer-shaped addends never sum to -0), a double otherwise.
fn emit_box_sum(
    ctx: &mut LowerCtx<'_>,
    sum: ValueId,
    both_int: IntShaped,
    slot: ValueId,
    done: BlockId,
) {
    let chk_blk = match both_int {
        IntShaped::No => return emit_box_double(ctx, sum, slot, done),
        IntShaped::Yes => ctx.cur_block,
        IntShaped::If(c) => {
            let chk = ctx.f.add_block();
            let dbl = ctx.f.add_block();
            branch(ctx, c, chk, dbl);
            ctx.cur_block = dbl;
            emit_box_double(ctx, sum, slot, done);
            chk
        }
    };
    ctx.cur_block = chk_blk;
    let int_blk = ctx.f.add_block();
    let dbl_blk = ctx.f.add_block();
    let n = inst(ctx, InstKind::FpToSi(Operand::Value(sum)), Type::I64);
    let back = inst(ctx, InstKind::SiToFp(Operand::Value(n)), Type::F64);
    let exact = inst(
        ctx,
        InstKind::FCmp(
            crate::ssa::FPred::Oeq,
            Operand::Value(back),
            Operand::Value(sum),
        ),
        Type::Bool,
    );
    let ge = inst(
        ctx,
        InstKind::ICmp(
            IPred::Sge,
            Operand::Value(n),
            Operand::ConstI64(i32::MIN as i64),
        ),
        Type::Bool,
    );
    let le = inst(
        ctx,
        InstKind::ICmp(
            IPred::Sle,
            Operand::Value(n),
            Operand::ConstI64(i32::MAX as i64),
        ),
        Type::Bool,
    );
    let in_range = inst(
        ctx,
        InstKind::BinOp(BinOp::And, Operand::Value(ge), Operand::Value(le)),
        Type::Bool,
    );
    let ok = inst(
        ctx,
        InstKind::BinOp(BinOp::And, Operand::Value(exact), Operand::Value(in_range)),
        Type::Bool,
    );
    branch(ctx, ok, int_blk, dbl_blk);

    ctx.cur_block = int_blk;
    let low = inst(
        ctx,
        bin(BinOp::And, n, Operand::ConstI64(0xFFFF_FFFF)),
        Type::I64,
    );
    let boxed = inst(
        ctx,
        bin(BinOp::Or, low, Operand::ConstI64(TAG_TYPE_NUMBER as i64)),
        Type::I64,
    );
    let v = inst(ctx, InstKind::IntToPtr(Operand::Value(boxed)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);

    ctx.cur_block = dbl_blk;
    emit_box_double(ctx, sum, slot, done);
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
fn emit_decode_number(
    ctx: &mut LowerCtx<'_>,
    bits: ValueId,
    slot: ValueId,
    int_slot: Option<ValueId>,
    done: BlockId,
) {
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
    store_int_shape(ctx, int_slot, true);
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
    store_int_shape(ctx, int_slot, false);
    store_and_join(ctx, Operand::Value(f), slot, done);
}

fn store_int_shape(ctx: &mut LowerCtx<'_>, int_slot: Option<ValueId>, int: bool) {
    if let Some(s) = int_slot {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(int), Operand::Value(s), 0),
        );
    }
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
