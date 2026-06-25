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
    if let Some(v) =
        crate::ssa_lower_str_arr_slice::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_str_arr_index_search::try_dispatch(ctx, &method, args, recv_op, recv_ty)
    {
        return Some(v);
    }
    None
}
