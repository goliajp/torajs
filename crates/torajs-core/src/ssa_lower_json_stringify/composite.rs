//! Composite-type arms (`Type::Arr` / `Type::Obj`) of
//! [`super::lower`], split out 2026-07-03 (fn-debt decomp). Bodies
//! verbatim from the pre-split match arms; element / field recursion
//! goes back through `super::lower`.

use crate::ssa::{ArrId, BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// §25.5.2 null gate shared by both composite arms — a NULL slot
/// (`Nullable<Arr>` exec/match miss, `Nullable<Obj>` holding JS
/// null) stringifies to `"null"`; the walk only runs on the non-null
/// branch (pre-fix the len / field loads dereferenced NULL, SIGSEGV;
/// same family as the 642 `json_quote_str` NULL arm on the Str
/// lane). The walked-result store reads `ctx.cur_block` AFTER the
/// walk, which may split blocks (656 lesson).
pub(super) fn with_null_gate(
    ctx: &mut LowerCtx,
    val_op: &Operand,
    slot_name: &str,
    walk: impl FnOnce(&mut LowerCtx) -> Operand,
) -> Operand {
    let out_slot = ctx.alloca_in_entry(Type::Str, Some(slot_name));
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, val_op.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let walk_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: walk_blk,
        },
    );
    ctx.cur_block = null_blk;
    let null_str = ctx.intern_string_literal("null");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(null_str), Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = walk_blk;
    let walked = walk(ctx);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(walked, Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// `[<e0>,<e1>,...]` — concat-loop accumulator over
/// `arr_layouts[arr_id]`-typed slots (see module doc of the parent),
/// behind the shared [`with_null_gate`] (655 arm).
pub(super) fn lower_arr(ctx: &mut LowerCtx, val_op: Operand, arr_id: ArrId) -> Operand {
    with_null_gate(ctx, &val_op.clone(), "__json_arr_out", |ctx| {
        lower_arr_walk(ctx, val_op, arr_id)
    })
}

/// The non-null walk body of [`lower_arr`] (verbatim pre-655 arm).
fn lower_arr_walk(ctx: &mut LowerCtx, val_op: Operand, arr_id: ArrId) -> Operand {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let arr_ptr = match val_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let lbrack = ctx.intern_string_literal("[");
    let rbrack = ctx.intern_string_literal("]");
    let comma = ctx.intern_string_literal(",");
    let acc = ctx.alloca_in_entry(Type::Str, Some("__json_arr"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lbrack), Operand::Value(acc), 0),
    );
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__json_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let in_bounds = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(in_bounds),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    let need_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(i_now), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let sep_blk = ctx.f.add_block();
    let no_sep_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(need_sep),
            then_blk: sep_blk,
            else_blk: no_sep_blk,
        },
    );
    ctx.cur_block = sep_blk;
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let with_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_now), Operand::Value(comma)],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_sep), Operand::Value(acc), 0),
    );
    // 642 ledger — release the pre-concat accumulator (str_concat
    // answers a fresh Str; the first-iteration "[" literal is a
    // FLAG_STATIC_LITERAL no-op through the same drop).
    ctx.emit_drop_value(Operand::Value(acc_now), Type::Str);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(no_sep_blk));
    ctx.cur_block = no_sep_blk;
    let (off_base, off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(i_now), 3, false);
    let elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, off_base.clone(), off),
        elem_ty,
        None,
    );
    let elem_str = super::lower(ctx, Operand::Value(elem), elem_ty);
    let acc_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let with_elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_now2), elem_str],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_elem), Operand::Value(acc), 0),
    );
    // 642 ledger — release the pre-concat accumulator and the
    // element's stringified temp (every per-elem lane answers a
    // fresh Str or an interned literal whose drop is a no-op).
    ctx.emit_drop_value(Operand::Value(acc_now2), Type::Str);
    ctx.emit_drop_value(elem_str.clone(), Type::Str);
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    let acc_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_final), Operand::Value(rbrack)],
        ),
        Type::Str,
        None,
    );
    // 642 ledger — release the final accumulator (a zero-element
    // array's acc is still the "[" literal: no-op drop).
    ctx.emit_drop_value(Operand::Value(acc_final), Type::Str);
    Operand::Value(result)
}
