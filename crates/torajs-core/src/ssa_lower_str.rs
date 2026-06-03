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
//! then, **no new lines may be added here** — per
//! `common/file-size.md` known-debt rule, debt items can only
//! shrink or hold; growing them is a HARD RULE violation.
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
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

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
    // `s.charCodeAt(LITERAL)` — inline a 4-byte LoadDyn
    // + mask to 0xff + zext, skipping the bounds check
    // and the runtime fn-call dispatch. Hot in tight
    // tokenize loops (RPN evaluator etc). Same shape as
    // emit_inline_str_eq_bytes — uses emit_str_data_base
    // to handle Str (base_off = 16) and Substr (base_off
    // = 16 + parent_offset, parent loaded once) uniformly.
    if matches!(recv_ty, Type::Str | Type::Substr)
        && matches!(method.as_str(), "charCodeAt" | "codePointAt")
        && args.len() == 1
        && let Expr::Number(n) = ctx.ast.get_expr(args[0])
        && *n >= 0.0
        && n.fract() == 0.0
        && (*n as i64) < 1024
    {
        let lit_idx = *n as i64;
        let (base, base_off) = ctx.emit_str_data_base(recv_op, recv_ty);
        let off_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, base_off, Operand::ConstI64(lit_idx)),
            Type::I64,
            None,
        );
        // Load 8 bytes (I64); little-endian byte at idx
        // is the low byte of the load → mask with 0xff
        // promotes to a clean I64 char code.
        let raw = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(Type::I64, base, Operand::Value(off_v)),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::And, Operand::Value(raw), Operand::ConstI64(0xff)),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    if recv_ty == Type::Substr && method == "charAt" && args.len() == 1 {
        // charAt on Substr: substr_slice(v, i, i+1).
        let idx_val = ctx.lower_expr(args[0]);
        let end = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_slice,
                vec![recv_op, idx_val, Operand::Value(end)],
            ),
            Type::Substr,
            None,
        );
        return Some(Operand::Value(v));
    }
    // V3-18 wedge — Substr.charAt() 0-arg variant —
    // hoisted above the generic Substr dispatch
    // because Substr.charAt has no view-aware
    // fast path and would otherwise hit the
    // 'unsupported Substr method' panic.
    if recv_ty == Type::Substr && method == "charAt" && args.is_empty() {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_slice,
                vec![recv_op, Operand::ConstI64(0), Operand::ConstI64(1)],
            ),
            Type::Substr,
            None,
        );
        return Some(Operand::Value(v));
    }
    if recv_ty == Type::Substr {
        // Step 13-a: stack-write trim fast-path (sibling).
        if let Some(v) =
            crate::ssa_lower_substr_trim_into::try_emit(ctx, &method, args.len(), recv_op, recv_ty)
        {
            return Some(v);
        }
        // View-aware fast paths — read bytes from
        // parent + offset directly, no per-call malloc.
        let view_aware = match method.as_str() {
            "charCodeAt" | "codePointAt" => Some((ctx.intrinsics.substr_char_code_at, Type::I64)),
            "startsWith" => Some((ctx.intrinsics.substr_starts_with, Type::Bool)),
            "endsWith" => Some((ctx.intrinsics.substr_ends_with, Type::Bool)),
            "includes" => Some((ctx.intrinsics.substr_includes, Type::Bool)),
            "indexOf" => Some((ctx.intrinsics.substr_index_of, Type::I64)),
            "slice" => Some((ctx.intrinsics.substr_slice, Type::Substr)),
            "substring" => Some((ctx.intrinsics.substr_substring, Type::Substr)),
            "trim" => Some((ctx.intrinsics.substr_trim, Type::Substr)),
            "trimStart" | "trimLeft" => Some((ctx.intrinsics.substr_trim_start, Type::Substr)),
            "trimEnd" | "trimRight" => Some((ctx.intrinsics.substr_trim_end, Type::Substr)),
            _ => None,
        };
        if let Some((target, ret_ty)) = view_aware {
            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(recv_op);
            for a in args {
                argv.push(ctx.lower_expr(*a));
            }
            // V3-18 m1.h.36 — Substr.slice / substring
            // also accept 0/1 args; fill defaults the
            // same way as the Str path. Substr len is
            // at offset 8 of the Substr layout.
            if matches!(method.as_str(), "slice" | "substring") && args.len() < 2 {
                if args.is_empty() {
                    argv.push(Operand::ConstI64(0));
                }
                let len = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::I64, recv_op, 8),
                    Type::I64,
                    None,
                );
                argv.push(Operand::Value(len));
            }
            // V3-18 wedge — Substr.charCodeAt /
            // codePointAt 0-arg defaults pos to 0.
            if matches!(method.as_str(), "charCodeAt" | "codePointAt") && args.is_empty() {
                argv.push(Operand::ConstI64(0));
            }
            let v = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
            return Some(Operand::Value(v));
        }
        match method.as_str() {
            // Unreachable now — view_aware above covers
            // these — but keep the explicit no-op match
            // arm in case the dispatch table later splits.
            "charCodeAt" | "codePointAt" => unreachable!(),
            // Methods producing new strings — materialize
            // first then route through the OWNED Str path.
            _ => {
                let owned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_to_owned, vec![recv_op]),
                    Type::Str,
                    None,
                );
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(Operand::Value(owned));
                for a in args {
                    argv.push(ctx.lower_expr(*a));
                }
                let (target, ret_ty) = match method.as_str() {
                    "slice" => (ctx.intrinsics.str_slice, Type::Str),
                    "substring" => (ctx.intrinsics.str_substring, Type::Str),
                    "toUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
                    "toLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
                    "trim" => (ctx.intrinsics.str_trim, Type::Str),
                    "trimStart" | "trimLeft" => (ctx.intrinsics.str_trim_start, Type::Str),
                    "trimEnd" | "trimRight" => (ctx.intrinsics.str_trim_end, Type::Str),
                    "padStart" => (ctx.intrinsics.str_pad_start, Type::Str),
                    "padEnd" => (ctx.intrinsics.str_pad_end, Type::Str),
                    "startsWith" => (ctx.intrinsics.str_starts_with, Type::Bool),
                    "endsWith" => (ctx.intrinsics.str_ends_with, Type::Bool),
                    "includes" => (ctx.intrinsics.str_includes, Type::Bool),
                    "indexOf" => (ctx.intrinsics.str_index_of, Type::I64),
                    "lastIndexOf" => (ctx.intrinsics.str_last_index_of, Type::I64),
                    "localeCompare" => (ctx.intrinsics.str_locale_compare, Type::I64),
                    // V3-18 wedge — Substr.search routes
                    // through str_index_of after
                    // materializing (same as Str.search):
                    // first match position or -1.
                    "search" => (ctx.intrinsics.str_index_of, Type::I64),
                    "at" => (ctx.intrinsics.str_at, Type::Str),
                    "repeat" => (ctx.intrinsics.str_repeat, Type::Str),
                    "replace" => (ctx.intrinsics.str_replace, Type::Str),
                    "replaceAll" => (ctx.intrinsics.str_replace_all, Type::Str),
                    other => {
                        panic!("ssa-lower: unsupported Substr method `{other}`")
                    }
                };
                let v =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
                // owned is consumed by the call (our Str
                // intrinsics are read-only on Str args, so
                // owned still needs scope-end drop — but
                // it's not bound to a local. Insert an
                // explicit drop so it doesn't leak).
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.str_drop, vec![Operand::Value(owned)]),
                );
                return Some(Operand::Value(v));
            }
        }
    }
    // V3-18 wedge — charAt / charCodeAt /
    // codePointAt 0-arg form per JS spec
    // §22.1.3.4 / §22.1.3.5 / §22.1.3.6: missing
    // pos defaults to 0. Synthesize a ConstI64(0)
    // index and route through the existing 1-arg
    // paths below.
    if matches!(recv_ty, Type::Str | Type::Substr)
        && matches!(method.as_str(), "charAt" | "charCodeAt" | "codePointAt")
        && args.is_empty()
    {
        let idx_val = Operand::ConstI64(0);
        if method == "charAt" {
            let v = if recv_ty == Type::Str {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                    Type::Substr,
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.substr_slice,
                        vec![recv_op, idx_val, Operand::ConstI64(1)],
                    ),
                    Type::Substr,
                    None,
                )
            };
            return Some(Operand::Value(v));
        }
        // charCodeAt / codePointAt — same intrinsic
        // (codePointAt collapses to charCodeAt in
        // tora's ASCII-only Str path).
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_char_code_at, vec![recv_op, idx_val]),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    // `s.charAt(i)` — same-shape alias for `s[i]`.
    // Lowers to a length-1 substr view instead of going
    // through a separate runtime helper.
    if matches!(recv_ty, Type::Str | Type::Substr) && method == "charAt" && args.len() == 1 {
        let idx_raw = ctx.lower_expr(args[0]);
        let idx_val = ctx.coerce_to_i64(idx_raw);
        let v = if recv_ty == Type::Str {
            // V3-18 m1.h.37 — bounds-checked str charAt.
            // Pre-fix called substr_create directly; OOB
            // indices stored garbage offsets and printed
            // bytes from past the parent's data.
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                Type::Substr,
                None,
            )
        } else {
            let end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.substr_slice,
                    vec![recv_op, idx_val, Operand::Value(end)],
                ),
                Type::Substr,
                None,
            )
        };
        return Some(Operand::Value(v));
    }
    /* v0.2 #6 — s.normalize() ASCII identity stub.
     * Returns a clone of the receiver via str_repeat
     * with N=1 (already-existing intrinsic). For ASCII
     * strings (the dominant test262 case) all four
     * NFC/NFD/NFKC/NFKD forms are byte-identical with
     * the input. Multi-byte UTF-8 normalization is
     * deferred to v1.0 with the rest of Unicode work. */
    if recv_ty == Type::Str && method == "normalize" {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.str_repeat,
                vec![recv_op, Operand::ConstI64(1)],
            ),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    // String methods.
    if recv_ty == Type::Str
        && matches!(
            method.as_str(),
            "slice"
                | "substring"
                | "substr"
                | "charCodeAt"
                | "codePointAt"
                | "startsWith"
                | "endsWith"
                | "includes"
                | "indexOf"
                | "split"
                | "repeat"
                | "toUpperCase"
                | "toLowerCase"
                | "trim"
                | "trimStart"
                | "trimEnd"
                | "trimLeft"
                | "trimRight"
                | "padStart"
                | "padEnd"
                | "replace"
                | "replaceAll"
                | "at"
                | "lastIndexOf"
                | "localeCompare"
                | "search"
        )
    {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(recv_op);
        for a in args {
            argv.push(ctx.lower_expr(*a));
        }
        // V3-18 m1.h.36 — String.slice / substring with
        // 0 or 1 args: fill in the missing positions
        // with start=0, end=str.length (per JS spec).
        if matches!(method.as_str(), "slice" | "substring") && args.len() < 2 {
            if args.is_empty() {
                argv.push(Operand::ConstI64(0));
            }
            // Read the receiver's length from the str
            // header (offset 8) — same shape as
            // s.length elsewhere.
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, 8),
                Type::I64,
                None,
            );
            argv.push(Operand::Value(len));
        }
        // T-49 — String.substr 0/1-arg defaults. substr's
        // 2nd arg is a *length* not an end index; missing
        // length means "remaining", which we encode as
        // i64::MAX so the runtime helper's
        // `length > avail ? avail` clamp picks it up.
        if method.as_str() == "substr" && args.len() < 2 {
            if args.is_empty() {
                argv.push(Operand::ConstI64(0));
            }
            argv.push(Operand::ConstI64(i64::MAX));
        }
        // V3-18 m1.h.45 — String.padStart / padEnd with 1
        // arg: default fill string is " " per JS spec
        // §21.1.3.16.
        if matches!(method.as_str(), "padStart" | "padEnd") && args.len() == 1 {
            let space = ctx.intern_string_literal(" ");
            argv.push(Operand::Value(space));
        }
        // V3-18 m1.h.50 — String.indexOf / lastIndexOf
        // with the 2-arg (needle, fromIndex) shape route
        // to the dedicated _from runtime helpers.
        if matches!(method.as_str(), "indexOf" | "lastIndexOf") && args.len() == 2 {
            let target = if method == "indexOf" {
                ctx.intrinsics.str_index_of_from
            } else {
                ctx.intrinsics.str_last_index_of_from
            };
            let v = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::Call(target, argv), Type::I64, None);
            return Some(Operand::Value(v));
        }
        // V3-18 m1.h.51 — startsWith / endsWith / includes
        // 2-arg (needle, position) shape: route to
        // dedicated _from helpers.
        if matches!(method.as_str(), "startsWith" | "endsWith" | "includes") && args.len() == 2 {
            let target = match method.as_str() {
                "startsWith" => ctx.intrinsics.str_starts_with_from,
                "endsWith" => ctx.intrinsics.str_ends_with_from,
                _ => ctx.intrinsics.str_includes_from,
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(target, argv),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(v));
        }
        let (target, ret_ty) = match method.as_str() {
            "slice" => (ctx.intrinsics.str_slice, Type::Str),
            "substring" => (ctx.intrinsics.str_substring, Type::Str),
            "substr" => (ctx.intrinsics.str_substr, Type::Str),
            "repeat" => (ctx.intrinsics.str_repeat, Type::Str),
            "toUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
            "toLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
            "trim" => (ctx.intrinsics.str_trim, Type::Str),
            "trimStart" | "trimLeft" => (ctx.intrinsics.str_trim_start, Type::Str),
            "trimEnd" | "trimRight" => (ctx.intrinsics.str_trim_end, Type::Str),
            "padStart" => (ctx.intrinsics.str_pad_start, Type::Str),
            "padEnd" => (ctx.intrinsics.str_pad_end, Type::Str),
            "replace" => (ctx.intrinsics.str_replace, Type::Str),
            "replaceAll" => (ctx.intrinsics.str_replace_all, Type::Str),
            "at" => (ctx.intrinsics.str_at, Type::Str),
            // `codePointAt` collapses to charCodeAt in tr's
            // byte-Str layout — both return the byte at
            // the index, indistinguishable inside the
            // ASCII / Latin-1 range tests stick to.
            "charCodeAt" | "codePointAt" => (ctx.intrinsics.str_char_code_at, Type::I64),
            "startsWith" => (ctx.intrinsics.str_starts_with, Type::Bool),
            "endsWith" => (ctx.intrinsics.str_ends_with, Type::Bool),
            "includes" => (ctx.intrinsics.str_includes, Type::Bool),
            "indexOf" => (ctx.intrinsics.str_index_of, Type::I64),
            // V3-18 wedge — String.prototype.search per
            // JS spec §22.1.3.16 with a string arg
            // (the RegExp arg form is a follow-up
            // substrate item alongside Symbol.search
            // dispatch). For a plain string the result
            // is exactly indexOf — first match position
            // or -1 — so route through the same helper.
            "search" => (ctx.intrinsics.str_index_of, Type::I64),
            "lastIndexOf" => (ctx.intrinsics.str_last_index_of, Type::I64),
            "localeCompare" => (ctx.intrinsics.str_locale_compare, Type::I64),
            "split" => {
                // Phase Substr.B — split returns
                // Array<Substr>: each output element
                // is a 32-byte view referencing the
                // source's bytes. Zero memcpy per
                // substring; hot loops over `expr.split(sep)`
                // pay only N small mallocs (no per-byte
                // copy). Downstream method dispatch on
                // Substr routes to view-aware intrinsics.
                // V3-18 wedge — `s.split(sep, limit)`:
                // call str_split, then arr_slice the
                // result to [0, min(limit, len)). Two
                // small mallocs (the split + the slice
                // header) but element views still alias
                // the source bytes, no per-substring
                // copy.
                let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Substr);
                if args.len() == 2 {
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
                    let limit_op = argv[2].clone();
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
                    return Some(Operand::Value(v));
                }
                (ctx.intrinsics.str_split, Type::Arr(arr_id))
            }
            _ => unreachable!(),
        };
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
        return Some(Operand::Value(v));
    }
    // Array<string>.join(sep) — receiver is Type::Arr,
    // method == "join". The check.rs guard ensures
    // element type is String, so we don't re-validate
    // here.
    // V3-18 wedge — Array.toString routes to the same
    // join intrinsic with sep="," per JS spec
    // §22.1.3.30. Same element-type constraint as
    // join itself.
    if let Type::Arr(elem_arr_id) = recv_ty
        && (method == "join" || method == "toString" || method == "toLocaleString")
    {
        let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
        // V3-18 m1.h.43 — element-type dispatch for
        // join. Number / Bool elements use dedicated
        // runtime helpers that ToString each element
        // inline; Str / Substr take the existing
        // pointer-walking helpers.
        let join_fid = match elem_ty {
            Type::Substr => ctx.intrinsics.arr_join_substr,
            Type::I64 => ctx.intrinsics.arr_join_i64,
            Type::F64 => ctx.intrinsics.arr_join_f64,
            Type::Bool => ctx.intrinsics.arr_join_bool,
            _ => ctx.intrinsics.arr_join,
        };
        let mut argv = Vec::with_capacity(2);
        argv.push(recv_op);
        // V3-18 m1.h.42 — default separator ","
        // when join() is called with no arg.
        let sep = if args.is_empty() {
            let s = ctx.intern_string_literal(",");
            Operand::Value(s)
        } else {
            ctx.lower_expr(args[0])
        };
        argv.push(sep);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(join_fid, argv),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    // `arr.flat()` / `arr.flat(N)` — N-level deep flatten.
    // Default depth = 1. Literal depth N is statically
    // unrolled into N calls to the depth-1 runtime
    // helper, peeling one Array<> layer per iter and
    // stopping early if a layer is non-Array. depth=0 is
    // a shallow clone via arr_slice.
    if let Type::Arr(_) = recv_ty
        && method == "flat"
        && args.len() <= 1
    {
        let depth: i64 = if args.is_empty() {
            1
        } else if let Expr::Number(d) = ctx.ast.get_expr(args[0]) {
            *d as i64
        } else {
            panic!("ssa-lower: flat depth must be a number literal");
        };
        if depth == 0 {
            // Shallow clone: arr_slice(recv, 0, len) +
            // per-element rc_inc on refcounted layouts.
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_slice,
                    vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
                ),
                recv_ty,
                None,
            );
            if let Type::Arr(arr_id) = recv_ty {
                let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
                if elem_ty.is_refcounted() {
                    let len2 = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    ctx.emit_arr_rc_inc_range(
                        Operand::Value(v),
                        Operand::ConstI64(0),
                        Operand::Value(len2),
                    );
                }
            }
            return Some(Operand::Value(v));
        }
        let mut cur = recv_op;
        let mut cur_ty = recv_ty;
        for _ in 0..depth {
            let Type::Arr(outer_id) = cur_ty else {
                break;
            };
            let outer_elem = ctx.arr_layouts[outer_id.0 as usize];
            let Type::Arr(_) = outer_elem else {
                break;
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_flat, vec![cur]),
                outer_elem,
                None,
            );
            cur = Operand::Value(v);
            cur_ty = outer_elem;
        }
        return Some(cur);
    }
    // `arr.sort(cmp)` — in-place insertion sort calling
    // `cmp` for each compare. Returns the same array. The
    // comparator's return is treated as an i64 (or
    // implicitly-promoted-to-i64); ssa-lower picks ICmp/
    // FCmp(>0) based on its actual SSA type. Insertion
    // sort is O(n²) but works for moderate array sizes
    // and avoids needing closure-aware C runtime.
    if let Type::Arr(arr_id) = recv_ty
        && (method == "sort" || method == "toSorted")
        && args.len() <= 1
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        // toSorted clones the receiver via arr_slice
        // before sorting so the source stays intact.
        // arr_slice does the alloc + memcpy in one
        // runtime call; the rest of the body operates on
        // the clone.
        let recv_op = if method == "toSorted" {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_slice,
                    vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
                ),
                Type::Arr(arr_id),
                None,
            );
            Operand::Value(v)
        } else {
            recv_op
        };
        let arr_ptr = match recv_op {
            Operand::Value(v) => v,
            _ => unreachable!(),
        };
        // V3-18 wedge — sort/toSorted with no comparator
        // emits inline element-type-aware `prev > cur`
        // instead of calling a user fn. cmp_val + cmp_ty
        // stay None for the no-arg path so the
        // pred-computation block can branch later.
        let (cmp_val, cmp_ty) = if let Some(arg0) = args.first() {
            let v = ctx.lower_expr(*arg0);
            let t = ctx.operand_ty(&v);
            (Some(v), Some(t))
        } else {
            (None, None)
        };
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let i_slot = ctx.alloca(Type::I64, Some("__sort_i"));
        // i = 1
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(1), Operand::Value(i_slot), 0),
        );
        let outer_hdr = ctx.f.add_block();
        let outer_body = ctx.f.add_block();
        let outer_after = ctx.f.add_block();
        ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
        // outer header: i < len?
        ctx.cur_block = outer_hdr;
        let i_now = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let in_outer = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(in_outer),
                then_blk: outer_body,
                else_blk: outer_after,
            },
        );
        // outer body: load cur = xs[i], j = i, then inner loop
        ctx.cur_block = outer_body;
        let i_now2 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        // T-13.5: head-aware byte offset for arr.sort() reads.
        let off_i = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(i_now2),
            3,
            false,
        );
        let cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_i),
            elem_ty,
            None,
        );
        let j_slot = ctx.alloca(Type::I64, Some("__sort_j"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(i_now2), Operand::Value(j_slot), 0),
        );
        // inner loop: while j > 0 && cmp(xs[j-1], cur) > 0: shift
        let inner_hdr = ctx.f.add_block();
        let inner_check = ctx.f.add_block();
        let inner_body = ctx.f.add_block();
        let inner_after = ctx.f.add_block();
        ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
        // inner header: j > 0?
        ctx.cur_block = inner_hdr;
        let j_now = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
            Type::I64,
            None,
        );
        let j_pos = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(j_now), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(j_pos),
                then_blk: inner_check,
                else_blk: inner_after,
            },
        );
        // inner check: load xs[j-1], call cmp, test > 0
        ctx.cur_block = inner_check;
        let j_minus_1 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(j_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        let off_jm1 = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
            3,
            false,
        );
        let prev = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_jm1.clone()),
            elem_ty,
            None,
        );
        // V3-18 wedge — branch on cmp_val presence.
        // With a user comparator: call it, test ret > 0.
        // Without: directly compare prev > cur using the
        // element-type-aware predicate (Sgt for I64,
        // Ogt for F64, str_locale_compare for Str).
        let pred_v = match (&cmp_val, &cmp_ty) {
            (Some(cv), Some(ct)) => {
                let cmp_ret = ctx.call_fn_value(
                    cv.clone(),
                    *ct,
                    vec![Operand::Value(prev), Operand::Value(cur)],
                );
                let cmp_ret_ty = ctx.f.value_type(cmp_ret);
                match cmp_ret_ty {
                    Type::F64 => ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::FCmp(FPred::Ogt, Operand::Value(cmp_ret), Operand::ConstF64(0.0)),
                        Type::Bool,
                        None,
                    ),
                    _ => ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(cmp_ret), Operand::ConstI64(0)),
                        Type::Bool,
                        None,
                    ),
                }
            }
            _ => match elem_ty {
                Type::F64 => ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::FCmp(FPred::Ogt, Operand::Value(prev), Operand::Value(cur)),
                    Type::Bool,
                    None,
                ),
                Type::Str | Type::Substr => {
                    let r = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.str_locale_compare,
                            vec![Operand::Value(prev), Operand::Value(cur)],
                        ),
                        Type::I64,
                        None,
                    );
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(r), Operand::ConstI64(0)),
                        Type::Bool,
                        None,
                    )
                }
                _ => ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(prev), Operand::Value(cur)),
                    Type::Bool,
                    None,
                ),
            },
        };
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(pred_v),
                then_blk: inner_body,
                else_blk: inner_after,
            },
        );
        // inner body: xs[j] = xs[j-1]; j--
        ctx.cur_block = inner_body;
        let off_j =
            ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_now), 3, false);
        // off_jm1 was computed in inner_check; recompute
        // here since this is a different block.
        let off_jm1_b = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
            3,
            false,
        );
        let prev2 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_jm1_b),
            elem_ty,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::StoreDyn(Operand::Value(prev2), Operand::Value(arr_ptr), off_j),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(j_minus_1), Operand::Value(j_slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
        // inner after: xs[j] = cur
        ctx.cur_block = inner_after;
        let j_final = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
            Type::I64,
            None,
        );
        let off_jf = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_final),
            3,
            false,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::StoreDyn(Operand::Value(cur), Operand::Value(arr_ptr), off_jf),
        );
        // i++
        let i_next = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now2), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
        ctx.cur_block = outer_after;
        return Some(Operand::Value(arr_ptr));
    }
    // `s.concat(...others)` — variadic string concat,
    // lowered as a left-fold over str_concat. Empty arg
    // list returns the receiver unchanged. The single-arg
    // case still flows through the typecheck Function-arm
    // dispatch but we intercept here uniformly to avoid
    // duplicate emit paths.
    if recv_ty == Type::Str && method == "concat" {
        if args.is_empty() {
            return Some(recv_op);
        }
        let mut acc = recv_op;
        for a in args {
            let other = ctx.lower_expr(*a);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_concat, vec![acc, other]),
                Type::Str,
                None,
            );
            acc = Operand::Value(v);
        }
        return Some(acc);
    }
    // `arr.concat(other)` — fresh array, single malloc +
    // two memcpys via the C runtime. Element type carried.
    // Phase B refcount: derived array's slots alias both
    // sources; inc each slot for non-Copy elements.
    //
    // V3-18 wedge — multi-arg form `xs.concat(a, b, ..., z)`
    // per JS spec §22.1.3.2 is supported by folding the
    // single-arg intrinsic left-to-right: each step's
    // result becomes the next step's receiver. Refcount
    // inc runs once at the end over the final array's
    // full length. Each intermediate also leaks otherwise;
    // those temporaries are drop-balanced by the rc-inc
    // window on the final result (intermediates aren't
    // bound to a name so the surrounding scope-end drop
    // doesn't see them).
    if let Type::Arr(arr_id) = recv_ty
        && method == "concat"
    {
        // 0-arg form ≡ shallow copy. Lower as
        // `arr_slice(recv, 0, len)` — the refcount-inc
        // walk below handles non-Copy elements the
        // same way as for slice / concat results.
        let mut acc = if args.is_empty() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_slice,
                    vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
                ),
                Type::Arr(arr_id),
                None,
            );
            Operand::Value(v)
        } else {
            recv_op
        };
        for a in args {
            let other = ctx.lower_expr(*a);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_concat, vec![acc, other]),
                Type::Arr(arr_id),
                None,
            );
            acc = Operand::Value(v);
        }
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, acc, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(acc, Operand::ConstI64(0), Operand::Value(len));
        }
        return Some(acc);
    }
    // `arr.at(i)` — element at i with negative-index wrap.
    // Inline SSA: idx = i < 0 ? len + i : i; load at idx.
    // Out-of-bounds is UB (matches the unchecked indexing
    // convention).
    if let Type::Arr(arr_id) = recv_ty
        && method == "at"
        && args.len() == 1
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        let i_val = ctx.lower_expr(args[0]);
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
    // `arr.copyWithin(target, start, end)` — in-place
    // memmove via runtime helper.
    //
    // V3-18 wedge — start / end are optional per JS
    // spec §22.1.3.3:
    //   xs.copyWithin(t)        = (t, 0, len)
    //   xs.copyWithin(t, s)     = (t, s, len)
    //   xs.copyWithin(t, s, e)  = (t, s, e)
    // Pre-fix the 3-arg branch was strict so check.rs's
    // fixed-arity rejected the shorter calls. Defaults
    // resolved here so the rc-aware path below sees
    // the canonical 3-arg form.
    //
    // Phase B refcount: for non-Copy elements the dst
    // range gets aliased ptrs from the src range. The
    // sequence MUST be inc-src-first then drop-dst,
    // otherwise an overlapping element could be freed
    // before its inc (use-after-free). Then arr_copy_within
    // does the bytewise memmove; refcounts now reflect
    // the post-copy slot ownership so the array's
    // eventual element-walk drop is balanced.
    if let Type::Arr(arr_id) = recv_ty
        && method == "copyWithin"
        && (1..=3).contains(&args.len())
    {
        // V3-18 wedge — copyWithin per JS spec
        // §22.1.3.3 normalises target / start / end
        // through ToIntegerOrInfinity, then for each
        // index n: if n < 0, n = max(len + n, 0); if
        // n >= len, n = len. Pre-fix tora used a
        // plain clamp_i64_to_range(0, len) which
        // mapped any negative input to 0, dropping
        // the canonical TS pattern of using
        // negatives to count from the end (e.g.
        // copyWithin(-2) shifts the tail to the
        // front).
        let len_for_norm = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let raw_target = ctx.lower_expr(args[0]);
        let target = ctx.relative_to_len(raw_target, Operand::Value(len_for_norm));
        let start = if args.len() >= 2 {
            let raw = ctx.lower_expr(args[1]);
            ctx.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() >= 3 {
            let raw = ctx.lower_expr(args[2]);
            ctx.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            // end = recv.length
            Operand::Value(len_for_norm)
        };
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            // Replicate arr_copy_within's clamp: lo = clamp(start),
            // hi = clamp(end), to = clamp(target), count = min(hi-lo,
            // len-to). Then inc src [lo, lo+count) and drop dst
            // [to, to+count) before the memmove.
            let len_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let len_op = Operand::Value(len_v);
            let lo = ctx.clamp_i64_to_range(start, Operand::ConstI64(0), len_op);
            let hi = ctx.clamp_i64_to_range(end, Operand::ConstI64(0), len_op);
            let to = ctx.clamp_i64_to_range(target, Operand::ConstI64(0), len_op);
            // raw_count = max(0, hi - lo)
            let diff = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Sub, hi, lo),
                Type::I64,
                None,
            );
            let raw_count =
                ctx.clamp_i64_to_range(Operand::Value(diff), Operand::ConstI64(0), len_op);
            // capacity left at dst = len - to
            let cap_left = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Sub, len_op, to),
                Type::I64,
                None,
            );
            // count = min(raw_count, cap_left), clamped >= 0
            let count =
                ctx.clamp_i64_to_range(raw_count, Operand::ConstI64(0), Operand::Value(cap_left));
            // src_end = lo + count, dst_end = to + count
            let src_end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, lo, count),
                Type::I64,
                None,
            );
            let dst_end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, to, count),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(recv_op, lo, Operand::Value(src_end));
            ctx.emit_arr_rc_drop_range(recv_op, elem_ty, to, Operand::Value(dst_end));
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_copy_within,
                vec![recv_op, target, start, end],
            ),
            Type::Arr(arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }
    // `arr.reverse()` — in-place over the receiver,
    // returns the same array pointer for chaining.
    if let Type::Arr(arr_id) = recv_ty
        && method == "reverse"
        && args.is_empty()
    {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_reverse, vec![recv_op]),
            Type::Arr(arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }
    // `arr.toReversed()` — non-mutating reverse. Fresh
    // alloc + reverse-direction slot copy via the C
    // runtime; original untouched. Phase B refcount:
    // for non-Copy elements, inc each derived slot to
    // share ownership with the source (see arr.slice
    // for rationale).
    if let Type::Arr(arr_id) = recv_ty
        && method == "toReversed"
        && args.is_empty()
    {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_to_reversed, vec![recv_op]),
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
    // `arr.with(i, v)` — non-mutating index update. The
    // C helper memcpy's the source array, then writes
    // `v` into the (negative-wrapped) `i` slot. Out-of-
    // bounds `i` is UB. Element value passed as i64 (the
    // 8-byte slot width); f64 elements would need a
    // bitcast not yet in the IR (matches `fill`).
    //
    // Phase B refcount: for non-Copy elements, the
    // derived array shares ownership of all non-`i`
    // slots with the source AND of slot `i` with the
    // caller's `v`. inc every slot uniformly — caller's
    // drop on `v` and source's per-slot drops balance
    // against the derived array's own per-slot drops.
    if let Type::Arr(arr_id) = recv_ty
        && method == "with"
        && args.len() == 2
    {
        let i_val = ctx.lower_expr(args[0]);
        let v_val = ctx.lower_expr(args[1]);
        let v_ty = ctx.operand_ty(&v_val);
        if v_ty == Type::F64 {
            panic!("ssa-lower: Array.with on f64 elements not yet supported (need IR bitcast)");
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_with, vec![recv_op, i_val, v_val]),
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
    // `arr.fill(v, start, end)` — uniform fill of the
    // [start, end) range. Element value is passed as
    // i64 (8-byte slot — works for i64 / Bool / Str /
    // Obj / Arr; f64 elements would need a bitcast not
    // yet in the IR). The intrinsic returns the same
    // pointer.
    //
    // Phase B refcount: for non-Copy elements, the
    // overwrite would leak the old value AND leave new-
    // value refcount imbalanced. Emit a per-slot SSA
    // loop that drops old + stores new + inc's new,
    // bypassing the C runtime for this case.
    if let Type::Arr(arr_id) = recv_ty
        && method == "fill"
        && (args.len() >= 1 && args.len() <= 3)
    {
        let value = ctx.lower_expr(args[0]);
        let value_ty = ctx.operand_ty(&value);
        if value_ty == Type::F64 {
            panic!("ssa-lower: Array.fill on f64 elements not yet supported (need IR bitcast)");
        }
        // V3-18 m1.h.53 — start defaults to 0, end
        // defaults to arr.length per JS spec §22.1.3.6.
        // V3-18 wedge — negatives count from the
        // end via relative_to_len (max(len + n, 0)
        // for n < 0, len for n >= len). The arr_fill
        // C runtime only does plain min/max clamp,
        // so the normalisation happens here so both
        // Copy and non-Copy paths see canonical
        // [0, len] indices.
        let len_for_norm = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let start = if args.len() >= 2 {
            let raw = ctx.lower_expr(args[1]);
            ctx.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() == 3 {
            let raw = ctx.lower_expr(args[2]);
            ctx.relative_to_len(raw, Operand::Value(len_for_norm))
        } else {
            Operand::Value(len_for_norm)
        };
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_copy() {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_fill, vec![recv_op, value, start, end]),
                Type::Arr(arr_id),
                None,
            );
            return Some(Operand::Value(v));
        }
        // Non-Copy fill: per-slot drop-old + store-new + inc-new.
        // Clamp [start, end) to [0, len] inline (matches arr_fill C semantics).
        let len_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let lo = ctx.clamp_i64_to_range(start, Operand::ConstI64(0), Operand::Value(len_v));
        let hi = ctx.clamp_i64_to_range(end, Operand::ConstI64(0), Operand::Value(len_v));
        let i_slot = ctx.alloca_in_entry(Type::I64, Some("__fill_i"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(lo, Operand::Value(i_slot), 0),
        );
        let header = ctx.f.add_block();
        let body = ctx.f.add_block();
        let after = ctx.f.add_block();
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
        ctx.cur_block = header;
        let i_now = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cond = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), hi),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: body,
                else_blk: after,
            },
        );
        ctx.cur_block = body;
        // T-13.5: head-aware offset for arr.fill loop.
        let off = ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(i_now), 3, false);
        let old = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, recv_op.clone(), off.clone()),
            elem_ty,
            None,
        );
        ctx.emit_drop_value(Operand::Value(old), elem_ty);
        ctx.f
            .append_void(ctx.cur_block, InstKind::StoreDyn(value, recv_op, off));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.rc_inc, vec![value]),
        );
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
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
        ctx.cur_block = after;
        return Some(recv_op);
    }
    // `arr.slice(start, end)` — fresh array of the
    // [start, end) range, single memcpy via
    // __torajs_arr_slice. Element type carried over
    // from the receiver. Phase B refcount: when the
    // element type is non-Copy (Str etc.), the derived
    // array's slots alias the source's; inc each slot's
    // refcount so the source and derived can both safely
    // walk-drop their elements.
    if let Type::Arr(arr_id) = recv_ty
        && method == "slice"
        && args.len() <= 2
    {
        // V3-18 m1.h.35 — JS spec §22.1.3.25 defaults:
        //   arr.slice()      = arr.slice(0, len)
        //   arr.slice(start) = arr.slice(start, len)
        // Read len once when needed; use it as the
        // default for the missing 2nd arg.
        let mut argv = Vec::with_capacity(3);
        argv.push(recv_op);
        let start = if args.is_empty() {
            Operand::ConstI64(0)
        } else {
            ctx.lower_expr(args[0])
        };
        argv.push(start);
        let end = if args.len() == 2 {
            ctx.lower_expr(args[1])
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
    if let Type::Arr(arr_id) = recv_ty
        && (method == "indexOf" || method == "lastIndexOf" || method == "includes")
        && (args.len() == 1 || args.len() == 2)
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
        if args.len() == 2 {
            let raw = ctx.lower_expr(args[1]);
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
                    let (tag, value): (i64, Operand) = match needle_ty {
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
