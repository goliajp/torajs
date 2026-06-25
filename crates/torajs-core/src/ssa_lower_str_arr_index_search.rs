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
//! ## CARVE-OUT — exceeds 500-LOC hard cap
//!
//! This file ships at ~530 LOC, over the file-size HARD RULE 500-LOC
//! cap. Reason: the inline scan loop is a single SSA emitter with
//! tightly-coupled fromIndex normalization (S217 undef / S331 Any
//! decode / +len wrap / clamp), bidirectional start/end derivation
//! (forward vs lastIndexOf reverse-as-forward), and the per-elem-ty
//! compare dispatch. A second-pass split along three axes is planned:
//!
//! - `needle_coerce.rs` — V3-18 needle ↔ elem-ty coerce (Type::I64/F64
//!   cross-pair, fractional / NaN const-fold short-circuit).
//! - `from_normalize.rs` — fromIndex normalize (raw → +len wrap →
//!   clamp; undef / Any decode; lastIndexOf end+1 upper-clamp).
//! - `compare_dispatch.rs` — per-elem-ty eq (F64 SameValueZero widen,
//!   Str / Any tag-recovery dispatch).
//!
//! Until then the file is carve-out tagged and its line count can
//! only shrink per [`common/file-size.md`] known-debt rule — no new
//! arms may be added.

#![allow(clippy::too_many_lines)]

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
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
        // V3-18 wedge — needle ↔ elem type coerce.
        // Pre-fix tora passed the raw needle straight
        // into the ICmp / FCmp dispatch, so an i64
        // array with an f64 needle (e.g. NaN, -0.0,
        // fractional) hit an LLVM verify error
        // ("Found FloatValue but expected IntValue").
        // JS spec §7.2.13 StrictEqualityComparison
        // treats both as Number, so the bridging
        // logic mirrors what the rest of tora does
        // for mixed-numeric BinOp.
        let needle = match (elem_ty, needle_ty) {
            (a, b) if a == b => needle_raw,
            (Type::I64, Type::F64) => {
                // Const f64 that's an integer in i64
                // range converts cleanly. Anything
                // else (NaN / Infinity / fractional)
                // can't equal any i64 element, so
                // short-circuit to -1 / false.
                if let Operand::ConstF64(n) = needle_raw
                    && n.is_finite()
                    && n.fract() == 0.0
                    && n >= i64::MIN as f64
                    && n <= i64::MAX as f64
                {
                    Operand::ConstI64(n as i64)
                } else if let Operand::ConstF64(_) = needle_raw {
                    if want_bool {
                        return Some(Operand::ConstBool(false));
                    }
                    return Some(Operand::ConstI64(-1));
                } else {
                    ctx.coerce_to_i64(needle_raw)
                }
            }
            (Type::F64, Type::I64) => ctx.coerce_to_f64(needle_raw),
            _ => needle_raw,
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
                // end_slot = clamp(eff + 1, 0, len)
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
                );
                let end_raw = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(SsaBinOp::Add, Operand::Value(eff), Operand::ConstI64(1)),
                    Type::I64,
                    None,
                );
                // upper-clamp to len
                let over = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(end_raw), Operand::Value(len_v)),
                    Type::Bool,
                    None,
                );
                let over_blk = ctx.f.add_block();
                let ok_blk = ctx.f.add_block();
                let join2 = ctx.f.add_block();
                ctx.f.set_term(
                    ctx.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(over),
                        then_blk: over_blk,
                        else_blk: ok_blk,
                    },
                );
                ctx.f.append_void(
                    over_blk,
                    InstKind::Store(Operand::Value(len_v), Operand::Value(end_slot), 0),
                );
                ctx.f.set_term(over_blk, Terminator::Br(join2));
                ctx.f.append_void(
                    ok_blk,
                    InstKind::Store(Operand::Value(end_raw), Operand::Value(end_slot), 0),
                );
                ctx.f.set_term(ok_blk, Terminator::Br(join2));
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
        let elem = if elem_ty == Type::Any {
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
            let off =
                ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(i_cur), 3, false);
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::LoadDyn(elem_ty, recv_op, off),
                elem_ty,
                None,
            )
        };
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
