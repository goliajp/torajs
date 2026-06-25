//! P11.1-S0 — String / Substr / Array `<recv>.<method>(args)` dispatch
//! carved out of `ssa_lower.rs::lower_expr_inner` (originally at
//! 19968-22146 = 2179 LOC, pre-extract).
//!
//! The dispatch lives here because P11.1 (Str byte → Hybrid Latin-1 /
//! UTF-16 internal representation, per RFC
//! `.claude/rfcs/20260603-p11-1-str-unicode-internal/`) will mutate
//! every method-lowering arm — pulling them out of the 28k-line
//! `lower_expr_inner` god-fn first lets each P11.1 sub-step (S1
//! layout, S3 runtime path, S4 charCodeAt/codePointAt split, S5
//! indexing semantics, ...) edit the right block without dragging
//! the surrounding god-fn into every diff.
//!
//! ## CARVE-OUT — god-file refactor transition state
//!
//! This file currently > 500 LOC (file-size HARD RULE soft-warns
//! at 300 / hard-limits at 500). Reason: this is a single-extract
//! commit that moves the full dispatch arm verbatim from
//! `ssa_lower.rs` so the god-file shrinks today, before P11.1's
//! substrate edits begin. Subsequent P11.1 sub-steps will split
//! this file into method-category sub-modules
//! (`ssa_lower_str/{view,search,transform,structural}.rs`). Until
//! then, **no new method-dispatch arms may be added** to the
//! extracted block — per `common/file-size.md` known-debt rule,
//! the dispatch table can only shrink or hold.
//!
//! ### Explicit P11.1 helper exception
//!
//! Layout-aware helpers added by P11.1 sub-steps (S1 → S7) may
//! live here when their purpose is to centralize an encoding-
//! aware dispatch that would otherwise be duplicated across
//! `ssa_lower.rs` call sites — e.g. [`load_str_or_substr_length`]
//! (S1). The net contract is that every such helper must keep
//! `ssa_lower.rs` strictly shrinking: adding `H` lines here is
//! only allowed when at least `H` lines drop out of the god-file
//! at the same time. Helpers live above the extracted-block
//! `try_lower_method_call` fn so the carve-out perimeter stays
//! auditable.
//!
//! ## Why a free-fn rather than a method on `LowerCtx`
//!
//! Matches the established sidekick pattern (`ssa_lower_process_on`,
//! `ssa_lower_substr_trim_into`, `ssa_lower_main_exit`, etc.) — each
//! is a `pub(crate) fn try_lower*(ctx: &mut LowerCtx, ...) -> Option<Operand>`
//! that the parent god-fn calls at the dispatch site. `LowerCtx`
//! internal items used here (`f`, `ast`, `cur_block`, `intrinsics`,
//! `arr_layouts` + a handful of helper methods) were promoted to
//! `pub(crate)` as part of this commit; no behavior change.

#![allow(clippy::too_many_lines)]
// CARVE-OUT: see module doc — this is a single-shot god-file
// extraction sized for sub-division in subsequent P11.1 sub-steps.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, FPred, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Load the `length` field off a Str / Substr operand and widen to
/// `i64` so the resulting [`Operand`] can flow into the i64-shaped
/// `Type::Number` lane that downstream SSA code expects.
///
/// Both branches now return a JS code-unit count:
/// - Str (post-P11.1-S1): `u32 @8`, zero-extended to i64.
/// - Substr (post-P11.1-S5): `u64 @8` loaded directly. The pre-S5
///   layout stored a byte count here; S5 flipped it to a code-unit
///   count so `.length` matches `bun` / spec on multi-byte source
///   strings (e.g. `"中文abc".charAt(1).length === 1` vs the pre-S5
///   `2`-byte-count answer).
///
/// Centralizing the load via this helper kept the S5 substrate
/// change a runtime-side flip rather than three matching IR-edit
/// sites (`.length` property access, inline `str === <literal>`,
/// `coerce_to_bool` of a non-null string).
///
/// Helper added 2026-06-03 as part of P11.1-S1 (acknowledged
/// extension of the sidekick's carve-out — see module doc).
pub(crate) fn load_str_or_substr_length(ctx: &mut LowerCtx, op: Operand, ty: Type) -> Operand {
    if ty == Type::Str {
        let raw = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I32, op, 8),
            Type::I32,
            None,
        );
        let widened = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ZExtI32ToI64(Operand::Value(raw)),
            Type::I64,
            None,
        );
        Operand::Value(widened)
    } else {
        // Substr (post-S5) — load the u64 code-unit count directly.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, op, 8),
            Type::I64,
            None,
        );
        Operand::Value(v)
    }
}

/// Try to lower `<recv>.<method>(args)` for a method whose name
/// belongs to the recognized String-stdlib slice (also covers
/// several Array / Substr methods that share the same name set).
///
/// Returns `Some(operand)` on a successful dispatch — the caller
/// should propagate it back as the lowered expression's result.
/// Returns `None` when the call shape does not match (e.g. callee
/// is not a member, or the method name isn't in the dispatch set,
/// or the receiver / arg types route somewhere else), so the
/// caller's `lower_expr_inner` falls through to the next dispatch
/// arm.
///
/// The body below is the verbatim ssa_lower.rs:20013-22145
/// excerpt with three mechanical rewrites:
///
/// 1. `self.` → `ctx.` (free-fn rather than method form)
/// 2. `*obj`  → `obj_eid` (the deref is consumed by the helper
///    signature and rebound as an owned `ExprId` for clarity)
/// 3. `return <expr>;` → `return Some(<expr>);` for every early
///    return inside the dispatch (33 occurrences). The implicit
///    fall-through at the end yields `None`.
pub(crate) fn try_lower_method_call(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj_eid, name) = match ctx.ast.get_expr(callee_eid) {
        Expr::Member { obj, name }
            if matches!(
                name.as_str(),
                "slice"
                    | "substring"
                    | "substr"
                    | "charCodeAt"
                    | "codePointAt"
                    | "charAt"
                    | "startsWith"
                    | "endsWith"
                    | "includes"
                    | "indexOf"
                    | "split"
                    | "join"
                    | "repeat"
                    | "toUpperCase"
                    | "toLowerCase"
                    | "toLocaleUpperCase"
                    | "toLocaleLowerCase"
                    | "trim"
                    | "trimStart"
                    | "trimEnd"
                    | "trimLeft"
                    | "trimRight"
                    | "padStart"
                    | "padEnd"
                    | "replace"
                    | "replaceAll"
                    | "reverse"
                    | "toReversed"
                    | "with"
                    | "fill"
                    | "at"
                    | "concat"
                    | "sort"
                    | "toSorted"
                    | "flat"
                    | "lastIndexOf"
                    | "localeCompare"
                    | "copyWithin"
                    | "normalize"
                    | "isWellFormed"
                    | "toWellFormed"
                    | "search"
                    | "toString"
                    | "toLocaleString"
            ) =>
        {
            (*obj, name.clone())
        }
        _ => return None,
    };
    // M6.1 — `s.method(args)` for the String stdlib slice.
    // Receiver must be Type::Str; methods route to the
    // matching __torajs_str_* runtime intrinsic. Args are
    // borrow-shaped (no consume — see the Call arm in
    // check.rs).
    let recv_op = ctx.lower_expr(obj_eid);
    let recv_ty = ctx.operand_ty(&recv_op);
    let method = name.clone();
    // Phase Substr.B: dispatch view-aware methods on
    // Type::Substr receivers without materializing.
    // Currently MVP routes only the cheap byte-only ops;
    // anything that needs string-shaped output goes
    // through to_owned + Str path (Phase D would add
    // direct-on-Substr variants for slice/substring).
    // P11.1-S2.4 — the pre-S2 `s.charCodeAt(LITERAL)` inline
    // (LoadDyn byte + mask 0xff + zext) assumed a Latin-1 byte-
    // stream payload. Post-S2 a UTF-16 Str's code unit is a
    // little-endian u16 at byte offset `idx × 2`, so the inline
    // would only return the low byte (e.g. 0x2D for `"中"`
    // instead of 0x4E2D). Re-introducing it as an
    // encoding-aware inline would need either a build-time
    // narrowing (Type::Str doesn't tell us the underlying
    // encoding) or a runtime header-flag read + branch — at
    // which point it's no cheaper than calling
    // `__torajs_str_char_code_at` directly. So the inline arm
    // is dropped and every charCodeAt / codePointAt routes
    // through the runtime intrinsic, which already does the
    // encoding-aware load.
    // S240 widens the 1-arg detection to 2-arg so the
    // charAt(idx, trailing) shape still routes through this fast
    // path; args[1] is never lowered (trailing-arg ignore).
    if let Some(v) =
        crate::ssa_lower_str_substr_dispatch::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_charat_wedges::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_short_circuits::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    // String methods.
    if let Some(v) =
        crate::ssa_lower_str_str_dispatch::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_join_flat::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_sort::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_concat_dispatch::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    // `arr.at(i?)` — element at i with negative-index wrap.
    // Inline SSA: idx = i < 0 ? len + i : i; load at idx.
    // Out-of-bounds is UB (matches the unchecked indexing
    // convention).
    //
    // Arity defaults per ES §23.1.3.1 step 2-3: undefined index
    // routes through ToIntegerOrInfinity → 0, so `arr.at()`
    // returns `arr[0]`. 0-arg form emits `i_val = ConstI64(0)`
    // and skips through the rest of the negative-wrap select.
    // S242 — Array<T>.at(idx, ...trailing) trailing-arg ignore per
    // ES §23.1.3.1: args[1..] is never read (i_val uses args[0] only).
    // S299 — widen upper-cap `args.len() <= 2` → no cap + lower-and-drop
    // args[1..] so step()-style side-effect exprs fire per ES eval-then-
    // discard semantics. Mirrors check.rs S299.
    if let Type::Arr(arr_id) = recv_ty
        && method == "at"
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        // ES §23.1.3.1 step 2 — ToIntegerOrInfinity on `index`. The
        // inline ICmp/Add/LoadDyn chain below is i64-only; without
        // this coerce, f64 args (`a.at(1.5)`) panic backend GPR
        // materialization. coerce_to_i64 const-folds NaN → 0 and
        // ±∞ → i64::{MAX,MIN} (spec), FpToSi otherwise.
        //
        // S225 — explicit `undefined` index lowers as a ConstPtrNull
        // which coerce_to_i64 cannot materialize (same shape as the
        // S222 charAt early-intercept). Short-circuit to ConstI64(0)
        // before coerce so the negative-wrap select picks arr[0].
        let i_val = if args.is_empty() {
            Operand::ConstI64(0)
        } else if matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        ) {
            Operand::ConstI64(0)
        } else {
            let raw = ctx.lower_expr(args[0]);
            ctx.coerce_to_i64(raw)
        };
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let is_neg = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, i_val, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let i_plus_len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, i_val, Operand::Value(len)),
            Type::I64,
            None,
        );
        // adj = is_neg ? i + len : i (via select-shape:
        // alloca + cond_br).
        let adj_slot = ctx.alloca_in_entry(Type::I64, Some("__at_idx"));
        let neg_blk = ctx.f.add_block();
        let pos_blk = ctx.f.add_block();
        let after_blk = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        ctx.f.append_void(
            neg_blk,
            InstKind::Store(Operand::Value(i_plus_len), Operand::Value(adj_slot), 0),
        );
        ctx.f.set_term(neg_blk, Terminator::Br(after_blk));
        ctx.f
            .append_void(pos_blk, InstKind::Store(i_val, Operand::Value(adj_slot), 0));
        ctx.f.set_term(pos_blk, Terminator::Br(after_blk));
        ctx.cur_block = after_blk;
        let adj = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(adj_slot), 0),
            Type::I64,
            None,
        );
        // T-13.5 deque: head-aware offset for arr.at(i).
        let off = ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(adj), 3, false);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, recv_op, off),
            elem_ty,
            None,
        );
        return Some(Operand::Value(v));
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_copy_within::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_inplace::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_fill::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    // `arr.slice(start, end)` — fresh array of the
    // [start, end) range, single memcpy via
    // __torajs_arr_slice. Element type carried over
    // from the receiver. Phase B refcount: when the
    // element type is non-Copy (Str etc.), the derived
    // array's slots alias the source's; inc each slot's
    // refcount so the source and derived can both safely
    // walk-drop their elements.
    // S242 — Array<T>.slice(start, end, ...trailing) trailing-arg
    // ignore per ES §23.1.3.28: args[2..] is never read; the end
    // branch below switches on `args.len() >= 2 && !arg1_undef`.
    // S299 — widen upper-cap `args.len() <= 3` → no cap + lower-and-drop
    // args[2..] so step()-style side-effect exprs fire per ES eval-then-
    // discard semantics. Mirrors check.rs S299.
    if let Type::Arr(arr_id) = recv_ty
        && method == "slice"
    {
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        // V3-18 m1.h.35 — JS spec §22.1.3.25 defaults:
        //   arr.slice()      = arr.slice(0, len)
        //   arr.slice(start) = arr.slice(start, len)
        // Read len once when needed; use it as the
        // default for the missing 2nd arg.
        let mut argv = Vec::with_capacity(3);
        argv.push(recv_op);
        // S213 — typed-Undefined for either operand defaults to
        // the spec value per ES §23.1.3.27 step 1-2: start
        // defaults to 0, end defaults to receiver length.
        let arg0_undef = args
            .first()
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Undefined)))
            .unwrap_or(false);
        let arg1_undef = args
            .get(1)
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Undefined)))
            .unwrap_or(false);
        // S334 — `xs.slice(Any [, Any])` per ES §23.1.3.28:
        // ToIntegerOrInfinity accepts arbitrary-typed input. Decode
        // Any via anyv_to_number → coerce_to_i64 so the helper's
        // (Arr, i64, i64) ABI sees clean i64s. Sister to S332/S333.
        let arg0_any = args
            .first()
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any)))
            .unwrap_or(false);
        let arg1_any = args
            .get(1)
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any)))
            .unwrap_or(false);
        let start = if args.is_empty() || arg0_undef {
            Operand::ConstI64(0)
        } else if arg0_any {
            let raw = ctx.lower_expr(args[0]);
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                Type::F64,
                None,
            );
            ctx.coerce_to_i64(Operand::Value(f))
        } else {
            ctx.lower_expr(args[0])
        };
        argv.push(start);
        let end = if args.len() >= 2 && !arg1_undef {
            if arg1_any {
                let raw = ctx.lower_expr(args[1]);
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                    Type::F64,
                    None,
                );
                ctx.coerce_to_i64(Operand::Value(f))
            } else {
                ctx.lower_expr(args[1])
            }
        } else {
            // Load receiver's len from offset 8.
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            Operand::Value(len)
        };
        argv.push(end);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_slice, argv),
            Type::Arr(arr_id),
            None,
        );
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(Operand::Value(v), Operand::ConstI64(0), Operand::Value(len));
        }
        return Some(Operand::Value(v));
    }
    // `arr.indexOf(needle)` / `arr.lastIndexOf(needle)` /
    // `arr.includes(needle)` — inline SSA loop. indexOf
    // returns the first match index (-1 on miss);
    // lastIndexOf scans from the end (-1 on miss);
    // includes returns a boolean. All three share the
    // per-element compare dispatch (ICmp / FCmp / str_eq).
    // ES §23.1.3.{14,17,18} — 0-arg `arr.indexOf() / .includes() /
    // .lastIndexOf()`: `searchElement` defaults to `undefined`. For
    // typed Array<T> with T ≠ Any, undefined cannot strictly equal
    // any element, so the result is fixed: indexOf/lastIndexOf → -1,
    // includes → false. Short-circuit before the full inline loop.
    // Array<Any> is left to the existing strict-arity reject (L3b —
    // needs an Any-tagged undefined sentinel needle).
    if let Type::Arr(arr_id) = recv_ty
        && (method == "indexOf" || method == "lastIndexOf" || method == "includes")
        && args.is_empty()
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty != Type::Any {
            if method == "includes" {
                return Some(Operand::ConstBool(false));
            }
            return Some(Operand::ConstI64(-1));
        }
    }
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
        let eq = match elem_ty {
            // ES §23.1.3.16: `includes` uses SameValueZero, which
            // treats NaN as equal to NaN. IEEE 754 `fcmp oeq` is
            // unordered-rejects-NaN, so plain `Oeq(NaN, NaN)` would
            // wrongly miss. `indexOf` / `lastIndexOf` keep
            // StrictEqualityComparison (NaN never matches) — so
            // only the `includes` arm widens to SameValueZero.
            // +0 / -0: IEEE 754 fcmp oeq treats them equal already,
            // matching SameValueZero's +0 === -0.
            Type::F64 if want_bool => {
                let eq_ord = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::FCmp(FPred::Oeq, Operand::Value(elem), needle.clone()),
                    Type::Bool,
                    None,
                );
                // x != x is true exactly when x is NaN (FPred::Une
                // = unordered or not equal; on a self-compare the
                // unordered bit is the only source of truth).
                let elem_nan = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::FCmp(FPred::Une, Operand::Value(elem), Operand::Value(elem)),
                    Type::Bool,
                    None,
                );
                let needle_nan = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::FCmp(FPred::Une, needle.clone(), needle),
                    Type::Bool,
                    None,
                );
                let both_nan = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::And,
                        Operand::Value(elem_nan),
                        Operand::Value(needle_nan),
                    ),
                    Type::Bool,
                    None,
                );
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Or,
                        Operand::Value(eq_ord),
                        Operand::Value(both_nan),
                    ),
                    Type::Bool,
                    None,
                )
            }
            Type::F64 => ctx.f.append_inst(
                ctx.cur_block,
                InstKind::FCmp(FPred::Oeq, Operand::Value(elem), needle),
                Type::Bool,
                None,
            ),
            Type::Str => ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_eq, vec![Operand::Value(elem), needle]),
                Type::Bool,
                None,
            ),
            // T-48 — Array<Any> per-element compare must go
            // through the boxed-any strict-eq helpers, not
            // raw ICmp. The elem is always a heap-Box ptr
            // (Type::Any); the needle may be either Any
            // (another box ptr) or a concrete primitive.
            // Pre-fix this arm fell through to ICmp(Ptr, I64)
            // when needle was a primitive, producing
            // "LLVM verify: Both operands to ICmp instruction
            // are not of the same type!" 3 cases under
            // test/built-ins/Array/prototype/{includes,
            // indexOf, lastIndexOf}/*.
            Type::Any => {
                if matches!(needle_ty, Type::Any) {
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.any_any_strict_eq,
                            vec![Operand::Value(elem), needle],
                        ),
                        Type::Bool,
                        None,
                    )
                } else {
                    // Pack needle as (tag, value-as-i64) — same
                    // shape as the BinOp Any === concrete arm.
                    // S127-1: `undefined` literal lowers to
                    // ConstPtrNull (Type::Ptr) — identical shape
                    // to the `null` literal. Recover the original
                    // AST distinction so strict_eq packs tag=5
                    // (ANY_UNDEF) instead of tag=0 (ANY_NULL).
                    // Without this, `xs.indexOf(undefined)` /
                    // `.lastIndexOf(undefined)` / `.includes(undefined)`
                    // wrongly match the first null slot — same
                    // ANY_UNDEF vs ANY_NULL collapse seen in W-D
                    // narrow trunk (S126-1/-3).
                    let is_undef_lit = matches!(
                        ctx.ast.get_expr(args[0]),
                        Expr::Ident(n) if n == "undefined"
                    );
                    let (tag, value): (i64, Operand) = if is_undef_lit {
                        (5, Operand::ConstI64(0))
                    } else {
                        match needle_ty {
                            Type::I64 | Type::I32 => (2, needle.clone()),
                            Type::F64 => {
                                let bits = ctx.f.append_inst(
                                    ctx.cur_block,
                                    InstKind::BitCastF64ToI64(needle.clone()),
                                    Type::I64,
                                    None,
                                );
                                (3, Operand::Value(bits))
                            }
                            Type::Bool => {
                                let zext = ctx.f.append_inst(
                                    ctx.cur_block,
                                    InstKind::ZExtBoolToI64(needle.clone()),
                                    Type::I64,
                                    None,
                                );
                                (1, Operand::Value(zext))
                            }
                            Type::Ptr if matches!(needle, Operand::ConstPtrNull) => {
                                (0, Operand::ConstI64(0))
                            }
                            t if t.is_refcounted() => (4, needle.clone()),
                            _ => (0, Operand::ConstI64(0)),
                        }
                    };
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.any_strict_eq,
                            vec![Operand::Value(elem), Operand::ConstI64(tag), value],
                        ),
                        Type::Bool,
                        None,
                    )
                }
            }
            _ => ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, Operand::Value(elem), needle),
                Type::Bool,
                None,
            ),
        };
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
