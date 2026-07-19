//! fromIndex normalization for the array index-search scan —
//! split out of [`crate::ssa_lower_str_arr_index_search`] (rotation
//! 147) when the `Arr<Any>` helper extraction pushed that file past
//! the 500-line limit.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// fromIndex normalization — S217 explicit-undefined → 0, S331 Any
/// decode via any_to_number, negative `+len` wrap with per-method
/// floor, and the lastIndexOf `end = clamp(eff+1, 0, len)` derivation
/// (split 2026-07-03, fn-debt decomp; body verbatim). Writes the
/// scan bounds into `i_slot` / `end_slot`.
pub(crate) fn normalize_from_index(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    want_last: bool,
    len_v: ValueId,
    i_slot: ValueId,
    end_slot: ValueId,
) {
    if args.len() >= 2 {
        // S217 — Array.{indexOf,lastIndexOf,includes}(needle,
        // undefined) per ES §22.1.3.{13,14,16}:
        // ToIntegerOrInfinity(undefined)=0. (Note: Array.lastIndexOf
        // differs from String.lastIndexOf — the *omitted* default is
        // len-1 but *explicit-undefined* still goes through
        // ToIntegerOrInfinity → 0; bun matches.) Short-circuit
        // arg[1]=undefined to ConstI64(0); the pos-path branch
        // below stores raw_i=0 into eff_slot, giving effective
        // fromIndex=0 across all three methods.
        let arg1_undef = matches!(
            ctx.expr_types.get(&args[1]),
            Some(crate::check::Type::Undefined)
        );
        let arg1_any = matches!(ctx.expr_types.get(&args[1]), Some(crate::check::Type::Any));
        let raw = if arg1_undef {
            Operand::ConstI64(0)
        } else if arg1_any {
            // S331 — `Array.{indexOf,lastIndexOf,includes}(needle,
            // Any fromIndex)` per ES §23.1.3.{14,17,18} step 4.
            // Decode Any via anyv_to_number → coerce_to_i64 below
            // (raw_i coerce sees F64 -> i64 fast path). Mirrors
            // S327 parseInt-Any-radix dispatch.
            let v = ctx.lower_expr(args[1]);
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![v]),
                Type::F64,
                None,
            );
            Operand::Value(f)
        } else {
            ctx.lower_expr(args[1])
        };
        let raw_i = ctx.coerce_to_i64(raw);
        let neg = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, raw_i, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let neg_blk = ctx.f.add_block();
        let pos_blk = ctx.f.add_block();
        let join_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        // neg path: effective = raw + len
        ctx.cur_block = neg_blk;
        let plus_len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, raw_i, Operand::Value(len_v)),
            Type::I64,
            None,
        );
        let pl_neg = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(plus_len), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        // For indexOf/includes: clamp negative effective to 0 (start).
        // For lastIndexOf:     clamp negative effective to -1 (so end=0).
        let neg_floor = if want_last { -1 } else { 0 };
        let zero_blk = ctx.f.add_block();
        let plus_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(pl_neg),
                then_blk: zero_blk,
                else_blk: plus_blk,
            },
        );
        let eff_slot = ctx.alloca_in_entry(Type::I64, Some("__eff"));
        ctx.f.append_void(
            zero_blk,
            InstKind::Store(Operand::ConstI64(neg_floor), Operand::Value(eff_slot), 0),
        );
        ctx.f.set_term(zero_blk, Terminator::Br(join_blk));
        ctx.f.append_void(
            plus_blk,
            InstKind::Store(Operand::Value(plus_len), Operand::Value(eff_slot), 0),
        );
        ctx.f.set_term(plus_blk, Terminator::Br(join_blk));
        // pos path: effective = raw_i
        ctx.f
            .append_void(pos_blk, InstKind::Store(raw_i, Operand::Value(eff_slot), 0));
        ctx.f.set_term(pos_blk, Terminator::Br(join_blk));
        ctx.cur_block = join_blk;
        let eff = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(eff_slot), 0),
            Type::I64,
            None,
        );
        if want_last {
            // spec §22.1.3.19 step 4: pos-path k = min(fromIndex, len-1);
            // end = k + 1 ≤ len. Branch on `eff >= len` BEFORE adding 1
            // so signed overflow never happens — pre-fix we computed
            // `eff + 1` first and only then clamped, which wrapped when
            // eff = i64::MAX (arr.lastIndexOf(v, Infinity) → fptosi
            // saturates to i64::MAX; +1 wraps to i64::MIN; `> len` is
            // false; end becomes negative; scan range [0, negative) is
            // empty; result is -1 even when a match exists).
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
            );
            let ge_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Sge, Operand::Value(eff), Operand::Value(len_v)),
                Type::Bool,
                None,
            );
            let ge_blk = ctx.f.add_block();
            let lt_blk = ctx.f.add_block();
            let join2 = ctx.f.add_block();
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(ge_len),
                    then_blk: ge_blk,
                    else_blk: lt_blk,
                },
            );
            // eff >= len path: end = len (equivalent to min(eff, len-1) + 1)
            ctx.f.append_void(
                ge_blk,
                InstKind::Store(Operand::Value(len_v), Operand::Value(end_slot), 0),
            );
            ctx.f.set_term(ge_blk, Terminator::Br(join2));
            // eff < len path: end = eff + 1 (safe — eff ≤ len - 1 ≤ i64::MAX - 1)
            let end_raw = ctx.f.append_inst(
                lt_blk,
                InstKind::BinOp(SsaBinOp::Add, Operand::Value(eff), Operand::ConstI64(1)),
                Type::I64,
                None,
            );
            ctx.f.append_void(
                lt_blk,
                InstKind::Store(Operand::Value(end_raw), Operand::Value(end_slot), 0),
            );
            ctx.f.set_term(lt_blk, Terminator::Br(join2));
            ctx.cur_block = join2;
        } else {
            // indexOf/includes: start = eff (already ≥ 0)
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(eff), Operand::Value(i_slot), 0),
            );
        }
    } else {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
        );
    }
}
