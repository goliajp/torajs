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
        // §23.1.3.17/.20 HasProperty gate — an Arr<Any> receiver can
        // carry HOLES (elision / deleted index), which indexOf and
        // lastIndexOf must skip; the inline loop reads slots
        // unconditionally. Route through the hole-aware runtime
        // kernels (exotic-header fast path inside keeps ordinary
        // arrays cheap). `includes` has no HasProperty step, and
        // typed element lanes cannot hold holes — both keep the
        // inline loop.
        if elem_ty == Type::Any && !want_bool {
            return Some(lower_any_index_of(ctx, args, recv_op, want_last));
        }
        let needle_raw = ctx.lower_expr(args[0]);
        let raw_ty = ctx.operand_ty(&needle_raw);
        // A needle from another comparison family comes back boxed,
        // so the type the compare dispatch sees is the helper's answer
        // and not `raw_ty`.
        let (needle, needle_ty) = match crate::ssa_lower_str_arr_index_coerce::coerce_needle(
            ctx, args[0], needle_raw, raw_ty, elem_ty, want_bool,
        ) {
            Ok(pair) => pair,
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
        crate::ssa_lower_str_arr_index_bounds::normalize_from_index(
            ctx, args, want_last, len_v, i_slot, end_slot,
        );
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
        // An owned-temp `any` needle (any-arith box) settles after
        // the scan — the boxed compare borrowed it per iteration
        // (mirror of `lower_any_index_of`'s release).
        if matches!(needle_ty, Type::Any) {
            ctx.release_owned_temp(args[0], &needle);
        }
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
/// §23.1.3.17/.20 on an `Arr<Any>` receiver — the HasProperty gate
/// means a hole must not answer a match, so the scan runs in the
/// runtime kernel rather than the inline loop below.
fn lower_any_index_of(
    ctx: &mut LowerCtx,
    args: &[ExprId],
    recv_op: Operand,
    want_last: bool,
) -> Operand {
    let needle_raw = ctx.lower_expr(args[0]);
    let needle_box = ctx.box_to_any_from_expr(args[0], needle_raw.clone());
    let from = if args.len() > 1 {
        let f_raw = ctx.lower_expr(args[1]);
        match ctx.operand_ty(&f_raw) {
            Type::I64 => f_raw,
            Type::F64 => ctx.coerce_to_i64(f_raw),
            Type::Any => {
                let n = ctx.coerce_any_to_number(f_raw, Type::F64);
                ctx.coerce_to_i64(n)
            }
            // Undefined and exotic shapes take the spec
            // defaults (indexOf 0 / lastIndexOf end).
            _ => {
                if want_last {
                    Operand::ConstI64(i64::MAX)
                } else {
                    Operand::ConstI64(0)
                }
            }
        }
    } else if want_last {
        Operand::ConstI64(i64::MAX)
    } else {
        Operand::ConstI64(0)
    };
    let fid = if want_last {
        ctx.intrinsics.arr_any_last_index_of
    } else {
        ctx.intrinsics.arr_any_index_of
    };
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![recv_op, needle_box, from]),
        Type::I64,
        None,
    );
    // The kernel borrows the needle box (a pure bit-encode
    // over the raw value — no stake of its own to release);
    // an owned-temp needle releases through its raw operand.
    let _ = needle_box;
    ctx.release_owned_temp(args[0], &needle_raw);
    Operand::Value(r)
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
