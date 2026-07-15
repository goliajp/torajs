//! `<Str>.split(sep?, limit?)` lowering — first carve-out chunk pulled
//! out of [`ssa_lower_str_str_dispatch::try_dispatch`] along the
//! structural-axis decomposition path. The 4-way
//! `view/search/transform/structural` per-method-family split
//! anticipated in the parent module doc is rejected because every
//! family shares the same undef-substitution / trailing-arg-drop
//! preamble — splitting by family would require duplicating that
//! preamble 4× and grow LOC rather than shrink it. The structural
//! axes (large self-contained arms like `split`, the trailing-drop
//! loop bodies, the default-fill block, the intrinsic lookup table)
//! decompose cleanly without preamble duplication.
//!
//! Handles three shapes — all of which return `Array<Substr>` so each
//! element is a 32-byte view aliasing the source's bytes (zero
//! per-substring copy, see Phase Substr.B):
//!
//! - 0-arg `s.split()` and 1-arg `s.split(undefined)` per ES
//!   §22.1.3.21 step 2-3: separator undef → return `[S]`, routed to
//!   the dedicated `str_split_no_sep` helper instead of falling back
//!   to a 1-arg `str_split` call with a ConstPtrNull sep slot (which
//!   SIGSEGV's the (Str, Str) ABI).
//! - 2+ arg `s.split(sep, limit, ...trailing)` per ES §22.1.3.21
//!   step 4-6: call `str_split`, then `arr_slice` the result to
//!   `[0, min(limit, len))`. `coerce_to_i64` on the limit const-folds
//!   NaN→0 / ±∞→i64::{MAX,MIN} so the downstream Slt/Store doesn't
//!   panic GPR materialization on fractional / NaN literals.
//!   Trailing args are already lower-and-dropped by the caller's
//!   S282 loop guard so `argv` stops at `[recv, sep, limit]`.
//! - 1-arg `s.split(sep)` with non-undef sep: emit the standard
//!   2-arg `Call(str_split, [recv, sep])` — the original arm's
//!   fall-through into the post-match Call site.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

/// Lower a `<Str>.split(...)` call. The caller (`ssa_lower_str_str_dispatch::try_dispatch`)
/// has already produced `argv = [recv, ...lowered_args]` with all
/// undef-substitution / trailing-arg-drop carve-outs applied.
pub(crate) fn lower_split(ctx: &mut LowerCtx<'_>, args: &[ExprId], argv: Vec<Operand>) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Substr);
    // S233 — `s.split(undefined)` per ES §22.1.3.21 step 2: separator
    // === undefined skips splitting altogether (step 3 returns `[S]`).
    // Without this carve-out the 1-arg-undef path falls through to
    // `str_split` with a ConstPtrNull sep slot, which SIGSEGV's the
    // helper's (Str, Str) ABI. Reroute to the same 0-arg helper.
    let split_1arg_undef = args.len() == 1
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
    if args.is_empty() || split_1arg_undef {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_split_no_sep, argv[..1].to_vec()),
            Type::Arr(arr_id),
            None,
        );
        return Operand::Value(v);
    }
    // 2+ arg with a Undefined separator — spec §22.1.3.21 steps 8-9:
    // ToUint32(limit) == 0 → return `[]`, else return `[S]`. Without
    // this arm the sep=undefined path falls through into the
    // `str_split(recv, sep_null_slot)` branch below and SIGSEGVs on
    // the (Str, Str) ABI (same failure mode as the 1-arg-undef guard
    // documented above).
    if args.len() >= 2
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        )
    {
        let no_sep_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_split_no_sep, argv[..1].to_vec()),
            Type::Arr(arr_id),
            None,
        );
        // limit === undefined → lim = 2^32-1 per spec step 6 → non-zero
        // → return [S] as-is. Short-circuit here so coerce_to_i64 (which
        // doesn't accept Ptr / ConstPtrNull) never sees the Undefined
        // literal (=ConstPtrNull for the frontend Type::Undefined lane).
        if matches!(
            ctx.expr_types.get(&args[1]),
            Some(crate::check::Type::Undefined)
        ) {
            return Operand::Value(no_sep_v);
        }
        let limit_op = ctx.coerce_to_i64(argv[2].clone());
        let is_zero = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, limit_op, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let is_zero_i = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ZExtBoolToI64(Operand::Value(is_zero)),
            Type::I64,
            None,
        );
        // take = 1 - (limit == 0 ? 1 : 0) — keeps to 1 for every
        // ToUint32 result but 0 (incl. negative literals whose i64
        // representation is non-zero; matches spec's step-8 gate).
        let take = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                SsaBinOp::Sub,
                Operand::ConstI64(1),
                Operand::Value(is_zero_i),
            ),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![
                    Operand::Value(no_sep_v),
                    Operand::ConstI64(0),
                    Operand::Value(take),
                ],
            ),
            Type::Arr(arr_id),
            None,
        );
        return Operand::Value(v);
    }
    // S282 — widen from `args.len() == 2` to `args.len() >= 2` so the
    // 3+ arg trailing-widen shape still drives the limit-clamp branch.
    // The S282 loop carve-out in the parent dispatcher lower-and-drops
    // args[2..] so `argv` stops at `[recv, sep, limit]`.
    if args.len() >= 2 {
        let split_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_split, argv[..2].to_vec()),
            Type::Arr(arr_id),
            None,
        );
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(split_v), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        // ES §22.1.3.21 step 6 — limit goes through ToUint32, which
        // truncates toward 0 for finite numbers. Without this coerce
        // a fractional / NaN / ±∞ literal stays f64 and the signed-int
        // ICmp/Store below panics backend GPR materialization.
        // `coerce_to_i64` const-folds NaN→0 / ±∞→i64::{MAX,MIN}
        // (downstream clamp picks len), trunc for finite f64.
        let limit_op = ctx.coerce_to_i64(argv[2].clone());
        let take_slot = ctx.alloca(Type::I64, Some("__split_take"));
        let lt = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, limit_op.clone(), Operand::Value(len)),
            Type::Bool,
            None,
        );
        let then_blk = ctx.f.add_block();
        let else_blk = ctx.f.add_block();
        let after_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(lt),
                then_blk,
                else_blk,
            },
        );
        ctx.cur_block = then_blk;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(limit_op, Operand::Value(take_slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
        ctx.cur_block = else_blk;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(len), Operand::Value(take_slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
        ctx.cur_block = after_blk;
        let take = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(take_slot), 0),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![
                    Operand::Value(split_v),
                    Operand::ConstI64(0),
                    Operand::Value(take),
                ],
            ),
            Type::Arr(arr_id),
            None,
        );
        return Operand::Value(v);
    }
    // 1-arg `s.split(sep)` with non-undef sep — standard 2-arg call.
    // This is the original arm's fall-through into the post-match Call
    // site (`(ctx.intrinsics.str_split, Type::Arr(arr_id))`).
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_split, argv),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(v)
}
