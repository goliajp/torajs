//! Inline number arms for `any` values: ToNumber of an any and `+`
//! with an any addend, the number encodings read and written through
//! the typed number nodes (`any_is_number`, `any_is_int32`,
//! `any_to_int32`, `any_to_f64`, `box_int32`, `box_f64`) and the
//! runtime call kept for every encoding that is not a number. The
//! egraph reads the number flow through the nodes and expands what
//! remains to the bit operations after φ promotion.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_anyv_inline::{branch, inst, store_and_join};

/// §7.1.4 ToNumber of an `any` value into an F64. An int32 or double
/// encoding decodes inline; everything else calls
/// `__torajs_anyv_to_number` and checks for a pending throw.
pub(crate) fn to_number(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let slot = ctx.alloca(Type::F64, None);
    let num_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();

    let is_num = inst(ctx, InstKind::AnyIsNumber(v), Type::Bool);
    branch(ctx, is_num, num_blk, slow_blk);

    ctx.cur_block = num_blk;
    let (f, _) = emit_decode_number(ctx, v);
    store_and_join(ctx, f, slot, done_blk);

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
/// and the sum is an integer in int32 range, `box_f64` otherwise.
/// Two int32 encodings add as integers, which gives the same box
/// without a float round trip. Any other value takes `slow`, which
/// must answer an owned `any`.
pub(crate) fn number_add(
    ctx: &mut LowerCtx<'_>,
    (a, a_ty): (Operand, Type),
    (b, b_ty): (Operand, Type),
    slow: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let res = ctx.alloca(Type::Any, None);
    let slow_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    match (a_ty, b_ty) {
        (Type::Any, Type::I64) => emit_any_int_add(ctx, (a, b), res, slow_blk, done_blk),
        (Type::I64, Type::Any) => emit_any_int_add(ctx, (b, a), res, slow_blk, done_blk),
        _ => emit_general_add(ctx, (a, a_ty), (b, b_ty), res, slow_blk, done_blk),
    }

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

/// Every `any` addend a number: the F64 sum, boxed per [`emit_box_sum`].
fn emit_general_add(
    ctx: &mut LowerCtx<'_>,
    (a, a_ty): (Operand, Type),
    (b, b_ty): (Operand, Type),
    res: ValueId,
    slow_blk: BlockId,
    done_blk: BlockId,
) {
    // past a failed int32 pair test the two `any` addends are never
    // both int32 encodings, so the sum boxes as a double
    let pair = a_ty == Type::Any && b_ty == Type::Any;
    if pair {
        let general = ctx.f.add_block();
        emit_int32_pair_add(ctx, (&a, &b), res, general, done_blk);
        ctx.cur_block = general;
    }
    let (l, l_int) = emit_number_side(ctx, &a, a_ty, slow_blk);
    let (r, r_int) = emit_number_side(ctx, &b, b_ty, slow_blk);
    let (l_int, r_int) = if pair {
        (IntShaped::No, IntShaped::No)
    } else {
        (l_int, r_int)
    };
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
}

/// An `any` addend `v` plus an I64 `n` (addition commutes, so either
/// source order lands here). An int32 encoding adds as an integer: the
/// sum is exact in i64 (`n` near the i64 edge never wraps into int32
/// range) and boxes as an int32 when it fits, else as the double the
/// float sum gives. A double encoding never makes the sum
/// integer-shaped, so it boxes as a double with no range test.
fn emit_any_int_add(
    ctx: &mut LowerCtx<'_>,
    (v, n): (Operand, Operand),
    slot: ValueId,
    slow: BlockId,
    done: BlockId,
) {
    let int_blk = ctx.f.add_block();
    let other_blk = ctx.f.add_block();
    let is_int = inst(ctx, InstKind::AnyIsInt32(v), Type::Bool);
    branch(ctx, is_int, int_blk, other_blk);

    ctx.cur_block = int_blk;
    let x = inst(ctx, InstKind::AnyToInt32(v), Type::I64);
    let sum = inst(
        ctx,
        InstKind::BinOp(BinOp::Add, Operand::Value(x), n),
        Type::I64,
    );
    let narrow = sext32(ctx, sum);
    let fits = inst(
        ctx,
        InstKind::ICmp(IPred::Eq, Operand::Value(narrow), Operand::Value(sum)),
        Type::Bool,
    );
    let fits_blk = ctx.f.add_block();
    let float_blk = ctx.f.add_block();
    branch(ctx, fits, fits_blk, float_blk);
    ctx.cur_block = fits_blk;
    let boxed = inst(ctx, InstKind::BoxInt32(Operand::Value(sum)), Type::Any);
    store_and_join(ctx, Operand::Value(boxed), slot, done);

    ctx.cur_block = other_blk;
    let is_num = inst(ctx, InstKind::AnyIsNumber(v), Type::Bool);
    branch(ctx, is_num, float_blk, slow);

    // a sum past int32 and a double encoding both add as floats
    ctx.cur_block = float_blk;
    let f = InstKind::AnyToF64(v, Operand::Value(is_int));
    let f = inst(ctx, f, Type::F64);
    let nf = inst(ctx, InstKind::SiToFp(n), Type::F64);
    let sum = InstKind::BinOp(BinOp::FAdd, Operand::Value(f), Operand::Value(nf));
    let sum = inst(ctx, sum, Type::F64);
    emit_box_double(ctx, sum, slot, done);
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
    let a_int = inst(ctx, InstKind::AnyIsInt32(*a), Type::Bool);
    let b_int = inst(ctx, InstKind::AnyIsInt32(*b), Type::Bool);
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
    let x = inst(ctx, InstKind::AnyToInt32(*a), Type::I64);
    let y = inst(ctx, InstKind::AnyToInt32(*b), Type::I64);
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
    let v = inst(ctx, InstKind::BoxInt32(Operand::Value(sum)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);

    ctx.cur_block = wide_blk;
    let f = inst(ctx, InstKind::SiToFp(Operand::Value(sum)), Type::F64);
    let v = inst(ctx, InstKind::BoxF64(Operand::Value(f)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);
}

/// The low 32 bits of `v`, sign-extended.
fn sext32(ctx: &mut LowerCtx<'_>, v: ValueId) -> ValueId {
    let hi = inst(
        ctx,
        InstKind::BinOp(BinOp::Shl, Operand::Value(v), Operand::ConstI64(32)),
        Type::I64,
    );
    inst(
        ctx,
        InstKind::BinOp(BinOp::AShr, Operand::Value(hi), Operand::ConstI64(32)),
        Type::I64,
    )
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
            let num_blk = ctx.f.add_block();
            let is_num = inst(ctx, InstKind::AnyIsNumber(*v), Type::Bool);
            branch(ctx, is_num, num_blk, slow);
            ctx.cur_block = num_blk;
            let (f, is_int) = emit_decode_number(ctx, *v);
            (f, IntShaped::If(is_int))
        }
    }
}

/// A known-number encoding as an F64, with the Bool that says it was
/// the int32 one.
fn emit_decode_number(ctx: &mut LowerCtx<'_>, v: Operand) -> (Operand, ValueId) {
    let is_int = inst(ctx, InstKind::AnyIsInt32(v), Type::Bool);
    let f = inst(
        ctx,
        InstKind::AnyToF64(v, Operand::Value(is_int)),
        Type::F64,
    );
    (Operand::Value(f), is_int)
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
    let v = inst(ctx, InstKind::BoxInt32(Operand::Value(n)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);

    ctx.cur_block = dbl_blk;
    emit_box_double(ctx, sum, slot, done);
}

/// `box_f64 f` into `slot`, then branch to `done`.
fn emit_box_double(ctx: &mut LowerCtx<'_>, f: ValueId, slot: ValueId, done: BlockId) {
    let v = inst(ctx, InstKind::BoxF64(Operand::Value(f)), Type::Any);
    store_and_join(ctx, Operand::Value(v), slot, done);
}
