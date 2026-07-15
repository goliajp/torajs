//! `<Arr>.indexOf(needle, from?)` / `<Arr>.lastIndexOf(needle, from?)` /
//! `<Arr>.includes(needle, from?)` main inline scan loop — twelfth
//! sub-split carved out of [`ssa_lower_str::try_lower_method_call`].
//!
//! Both directions are folded onto a single forward-scan loop over
//! `[start_slot, end_slot)` that either breaks on the first match
//! (`indexOf` / `includes`) or records the last match and keeps going
//! (`lastIndexOf`). Per-element compare dispatches off `elem_ty`:
//! `Type::F64` (with `includes` widening to SameValueZero for NaN /
//! ±0), `Type::Str` (`__torajs_str_eq`), `Type::Any` (boxed strict-eq
//! helpers, including the S127-1 `undefined` literal ANY_UNDEF tag
//! recovery), default `ICmp::Eq`.
//!
//! Args[2..] are lower-and-dropped per S278 (trailing-arg ignore,
//! ES §23.1.3.{14,16,17}). Zero-arg short-circuit (`args.is_empty()`)
//! is handled by [`super::ssa_lower_str_arr_slice::try_dispatch`].
//!
//! Returns `None` for non-Arr receivers, non-matching methods, or the
//! zero-arg case so the caller can keep trying the remaining branches.
//!
//! 2026-07-03 fn-debt decomp: the fromIndex-normalize block (the
//! `from_normalize` axis planned in the original carve-out doc)
//! splits to the file-local [`normalize_from_index`] fn and the
//! per-element load to [`emit_elem_load`]; `try_dispatch` keeps the
//! scan-loop skeleton. (The former over-500 carve-out note is
//! obsolete — the coerce / eq axes already live in the
//! `_index_coerce` / `_index_eq` siblings and the file is back
//! under the cap.)

#![allow(clippy::too_many_lines)]

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.indexOf(needle, from?)` /
/// `<Arr>.lastIndexOf(needle, from?)` / `<Arr>.includes(needle, from?)`.
/// Returns `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.indexOf(needle)` / `arr.lastIndexOf(needle)` /
    // `arr.includes(needle)` — inline SSA loop. indexOf
    // returns the first match index (-1 on miss);
    // lastIndexOf scans from the end (-1 on miss);
    // includes returns a boolean. All three share the
    // per-element compare dispatch (ICmp / FCmp / str_eq).
    // (Zero-arg `args.is_empty()` short-circuit lives in
    // `ssa_lower_str_arr_slice::try_dispatch`.)
    if let Type::Arr(arr_id) = recv_ty
        && (method == "indexOf" || method == "lastIndexOf" || method == "includes")
        && !args.is_empty()
    {
        let want_bool = method == "includes";
        let want_last = method == "lastIndexOf";
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        let needle_raw = ctx.lower_expr(args[0]);
        let needle_ty = ctx.operand_ty(&needle_raw);
        let needle = match crate::ssa_lower_str_arr_index_coerce::coerce_needle(
            ctx, needle_raw, needle_ty, elem_ty, want_bool,
        ) {
            Ok(op) => op,
            Err(short_circuit) => return Some(short_circuit),
        };
        let result_slot = ctx.alloca_in_entry(Type::I64, Some("__idx"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(-1), Operand::Value(result_slot), 0),
        );
        let len_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        // V3-18 m1.h.49 + lastIndexOf-from wedge.
        // Per JS spec §22.1.3.13 / §22.1.3.16:
        //   indexOf(needle, from?)     forward,  start = from
        //   includes(needle, from?)    forward,  start = from
        //   lastIndexOf(needle, from?) reverse,  start = from
        //
        // Both directions are folded onto a single
        // forward-scan loop over [start_slot, end_slot)
        // that records the *last* match (for lastIndexOf)
        // or breaks on the first match (indexOf/includes):
        //   indexOf/includes : start=normalized(from?), end=len
        //   lastIndexOf      : start=0, end=normalized(from?)+1
        // For lastIndexOf, scanning forward over [0, from+1)
        // and keeping the last match is equivalent to the
        // spec's reverse walk from `from` down to 0.
        let i_slot = ctx.alloca_in_entry(Type::I64, Some("__i"));
        let end_slot = ctx.alloca_in_entry(Type::I64, Some("__end"));
        // default end = len
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(len_v), Operand::Value(end_slot), 0),
        );
        normalize_from_index(ctx, args, want_last, len_v, i_slot, end_slot);
        // S278 — Array.{indexOf,lastIndexOf,includes}(needle, fromIndex,
        // ...trailing) trailing-arg ignore per ES §23.1.3.{14,16,17}.
        // The scan helper only reads needle + fromIndex; lower-and-drop
        // args[2..] so step()-style side-effect exprs fire per ES eval-
        // then-discard semantics. Same S272/S275/S277 idiom — silent-
        // drop would violate trailing-arg eval-then-discard.
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        let header = ctx.f.add_block();
        let body = ctx.f.add_block();
        let after = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(header));
        ctx.cur_block = header;
        let i_cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let end_cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(end_slot), 0),
            Type::I64,
            None,
        );
        let in_bounds = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_cur), Operand::Value(end_cur)),
            Type::Bool,
            None,
        );
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(in_bounds),
                then_blk: body,
                else_blk: after,
            },
        );
        ctx.cur_block = body;
        let elem = emit_elem_load(ctx, elem_ty, recv_op, i_cur);
        let eq = crate::ssa_lower_str_arr_index_eq::emit_compare(
            ctx, elem, elem_ty, needle, needle_ty, want_bool, args[0],
        );
        let found = ctx.f.add_block();
        let next = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(eq),
                then_blk: found,
                else_blk: next,
            },
        );
        ctx.cur_block = found;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(i_cur), Operand::Value(result_slot), 0),
        );
        let cb = ctx.cur_block;
        // indexOf / includes break on first match;
        // lastIndexOf keeps going so the result_slot
        // ends up holding the highest matching index.
        if want_last {
            ctx.f.set_term(cb, Terminator::Br(next));
        } else {
            ctx.f.set_term(cb, Terminator::Br(after));
        }
        ctx.cur_block = next;
        let next_i = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_cur), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(next_i), Operand::Value(i_slot), 0),
        );
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(header));
        ctx.cur_block = after;
        let _ = arr_id;
        let r = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(result_slot), 0),
            Type::I64,
            None,
        );
        if want_bool {
            // `includes` — return (result_slot != -1) as Bool.
            let b = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(-1)),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(b));
        }
        return Some(Operand::Value(r));
    }
    None
}

/// fromIndex normalization — S217 explicit-undefined → 0, S331 Any
/// decode via any_to_number, negative `+len` wrap with per-method
/// floor, and the lastIndexOf `end = clamp(eff+1, 0, len)` derivation
/// (split 2026-07-03, fn-debt decomp; body verbatim). Writes the
/// scan bounds into `i_slot` / `end_slot`.
fn normalize_from_index(
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

/// One `xs[i]` element load for the scan loop — Any-tagged slots go
/// through the arr_get_any_tag / _value / any_box dance, everything
/// else through the 8-byte-stride LoadDyn (split 2026-07-03,
/// fn-debt decomp; body verbatim incl. the T-13.5 / T-48 comment).
fn emit_elem_load(
    ctx: &mut LowerCtx<'_>,
    elem_ty: Type,
    recv_op: Operand,
    i_cur: ValueId,
) -> ValueId {
    // T-13.5: head-aware offset for indexOf-style scan.
    // T-48 — Array<Any> slots are 16-byte tagged
    // (tag,value) pairs. The 8-byte-stride LoadDyn
    // path below only matches I64/F64/Str arrays; for
    // Any we go through the same arr_get_any_tag /
    // _value / any_box dance the regular `xs[i]` read
    // uses (P1.4). This per-iteration alloc is the
    // same trade-off Index read accepts — performance
    // can come later via a fused includes helper if
    // it shows up in profiles.
    if elem_ty == Type::Any {
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_tag,
                vec![recv_op.clone(), Operand::Value(i_cur)],
            ),
            Type::I64,
            None,
        );
        let value = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_value,
                vec![recv_op.clone(), Operand::Value(i_cur)],
            ),
            Type::I64,
            None,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::Value(tag), Operand::Value(value)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (off_base, off) =
            ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(i_cur), 3, false);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_base.clone(), off),
            elem_ty,
            None,
        )
    }
}
