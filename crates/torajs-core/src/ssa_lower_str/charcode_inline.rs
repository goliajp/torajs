//! Inline `Str.charCodeAt(i)` (ES §22.1.3.2) — the owned-string
//! sibling of the Substr wedge in
//! [`crate::ssa_lower_str_charat_wedges`].
//!
//! The kernel call priced at one cross-archive BL per code unit on
//! every tokenizer-shaped loop, while the read itself is a length
//! load, an unsigned bound test, a flags test and one byte or
//! halfword load. Two forms:
//!
//! - unproven index: bound test first, NaN out of range (step 5),
//!   then the Latin-1 byte or the UTF-16 halfword; the value is F64
//!   because NaN has to be representable;
//! - index the width walk proved inside `[0, length)` — an enclosing
//!   `i < s.length` guard over a counter that is never negative,
//!   `WidthTable::char_code_at_proven` — has no NaN arm, so the code
//!   unit is answered as the I64 the load produces, and the width the
//!   walk gave the value's consumers is I64 too. The two decisions
//!   are one judgment: the walk records the call only where it also
//!   narrowed the width, and the lowering narrows only what the walk
//!   recorded.
//!
//! Layout (torajs-str `layout.rs`): flags u16 @6 carries
//! `STR_FLAG_IS_LATIN1` (bit 1, bit 49 of the packed header u64),
//! length u32 @8, payload @16 — Latin-1 one byte per code unit,
//! UTF-16 one little-endian u16.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// `<Str>.charCodeAt(i)` with exactly one argument. Leaves
/// `cur_block` on the join.
pub(crate) fn try_inline_str_char_code_at(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    args: &[ExprId],
    recv_op: Operand,
) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let idx = lower_index(ctx, args[0]);
    if ctx.num_f64_slots.char_code_at_proven(call_eid) {
        return Some(emit_code_unit(ctx, recv_op, idx));
    }
    let cur = ctx.cur_block;
    let result_slot = ctx.alloca(Type::F64, Some("__ccat_f64"));
    let len = crate::ssa_lower_str::load_str_or_substr_length(ctx, recv_op, Type::Str);
    let in_bounds = ctx.cmp(IPred::Ult, idx, len);
    let inb_blk = ctx.f.add_block();
    let oob_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: in_bounds,
            then_blk: inb_blk,
            else_blk: oob_blk,
        },
    );
    ctx.f.append_void(
        oob_blk,
        InstKind::Store(Operand::ConstF64(f64::NAN), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(oob_blk, Terminator::Br(join_blk));

    ctx.cur_block = inb_blk;
    let unit = emit_code_unit(ctx, recv_op, idx);
    let f = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::SiToFp(unit), Type::F64, None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(f), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));

    ctx.cur_block = join_blk;
    let out = ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::F64, Operand::Value(result_slot), 0),
        Type::F64,
        None,
    );
    Some(Operand::Value(out))
}

/// A proven `charCodeAt` call that some other lane answered as an
/// F64 (a split-view part, a Substr receiver) is narrowed to the
/// I64 the width walk promised its consumers; in range the f64 is an
/// exact integer, so the truncation is the identity.
pub(crate) fn narrow_proven(ctx: &mut LowerCtx<'_>, call_eid: ExprId, v: Operand) -> Operand {
    if ctx.operand_ty(&v) != Type::F64 || !ctx.num_f64_slots.char_code_at_proven(call_eid) {
        return v;
    }
    ctx.coerce_to_i64(v)
}

/// The index slot the way the kernel lane reads it: a static
/// `undefined` is 0 (step 2-3), a Number is truncated (§7.1.5, the
/// `fcvtzs` the call boundary would apply), anything else runs the
/// runtime's ToNumber first.
fn lower_index(ctx: &mut LowerCtx<'_>, a: ExprId) -> Operand {
    match ctx.expr_types.get(&a) {
        Some(crate::check::Type::Undefined) => Operand::ConstI64(0),
        Some(crate::check::Type::Number) => {
            let n = ctx.lower_expr(a);
            ctx.coerce_to_i64(n)
        }
        _ => ctx.lower_to_index_operand(a),
    }
}

/// The code unit at an in-range `idx`, as an I64: the Latin-1 byte
/// at `recv + 16 + idx` or the UTF-16 halfword at `recv + 16 +
/// idx*2`, chosen on the header's encoding flag. Leaves `cur_block`
/// on the join.
fn emit_code_unit(ctx: &mut LowerCtx<'_>, recv_op: Operand, idx: Operand) -> Operand {
    let cur = ctx.cur_block;
    let result_slot = ctx.alloca(Type::I64, Some("__ccat_unit"));
    let hdr = ctx
        .f
        .append_inst(cur, InstKind::Load(Type::I64, recv_op, 0), Type::I64, None);
    let flag = ctx.bin(
        SsaBinOp::And,
        Operand::Value(hdr),
        Operand::ConstI64(0x0002i64 << 48),
        Type::I64,
    );
    let is_lat = ctx.cmp(IPred::Ne, flag, Operand::ConstI64(0));
    let lat_blk = ctx.f.add_block();
    let wide_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: is_lat,
            then_blk: lat_blk,
            else_blk: wide_blk,
        },
    );

    ctx.cur_block = lat_blk;
    let j = ctx.bin(SsaBinOp::Add, idx, Operand::ConstI64(16), Type::I64);
    let b = ctx
        .f
        .append_inst(lat_blk, InstKind::LoadU8Dyn(recv_op, j), Type::I64, None);
    ctx.f.append_void(
        lat_blk,
        InstKind::Store(Operand::Value(b), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(lat_blk, Terminator::Br(join_blk));

    // the halfword load scales its index by 2, so the 16-byte header
    // is 8 halfwords
    ctx.cur_block = wide_blk;
    let j = ctx.bin(SsaBinOp::Add, idx, Operand::ConstI64(8), Type::I64);
    let u = ctx
        .f
        .append_inst(wide_blk, InstKind::LoadU16Dyn(recv_op, j), Type::I64, None);
    ctx.f.append_void(
        wide_blk,
        InstKind::Store(Operand::Value(u), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(wide_blk, Terminator::Br(join_blk));

    ctx.cur_block = join_blk;
    let out = ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::I64, Operand::Value(result_slot), 0),
        Type::I64,
        None,
    );
    Operand::Value(out)
}
