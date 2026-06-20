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
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

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
    if recv_ty == Type::Substr && method == "charAt" && !args.is_empty() {
        // charAt on Substr: substr_slice(v, i, i+1).
        // S272 — widen `== 1 || == 2` to `>= 1`; trailing exprs eval-
        // and-drop below per ES §22.1.3.2 trailing-arg ignore.
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
        for &a in &args[1..] {
            let _ = ctx.lower_expr(a);
        }
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
            "charCodeAt" => Some((ctx.intrinsics.substr_char_code_at, Type::I64)),
            "codePointAt" => Some((ctx.intrinsics.substr_code_point_at, Type::I64)),
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
                    // S140 — Locale variants alias to the non-locale
                    // form (en-US default host locale, matches bun).
                    "toUpperCase" | "toLocaleUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
                    "toLowerCase" | "toLocaleLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
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
                // ES §22.1.3.16 step 4 — `s.repeat(n < 0)` throws
                // RangeError. Runtime sets a TLS pending throw;
                // emit_throw_check propagates here.
                if method == "repeat" {
                    ctx.emit_throw_check(None);
                }
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
        // P11.3-A1 — split charCodeAt vs codePointAt (the latter
        // combines surrogate pairs per ES §22.1.3.3). 0-arg form
        // still applies: `'😀'.codePointAt()` should default pos
        // to 0 and return 0x1F600, not 0xD83D.
        let target = if method == "codePointAt" {
            if recv_ty == Type::Str {
                ctx.intrinsics.str_code_point_at
            } else {
                ctx.intrinsics.substr_code_point_at
            }
        } else if recv_ty == Type::Str {
            ctx.intrinsics.str_char_code_at
        } else {
            ctx.intrinsics.substr_char_code_at
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, vec![recv_op, idx_val]),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    // `s.charAt(i)` — same-shape alias for `s[i]`.
    // Lowers to a length-1 substr view instead of going
    // through a separate runtime helper.
    // S240 widens the 1-arg detection to 2-arg so the
    // charAt(idx, trailing) shape still routes through this length-1
    // substr-view fast path; args[1] is never lowered (trailing-arg
    // ignore).
    //
    // S272 — widen `== 1 || == 2` to `>= 1` so charAt(idx, ...trailing)
    // with any trailing count routes through this fast path; the
    // trailing exprs are eval-and-dropped below so side effects fire.
    if matches!(recv_ty, Type::Str | Type::Substr) && method == "charAt" && !args.is_empty() {
        // S222 — `s.charAt(undefined)` per ES §22.1.3.2 step 2-3:
        // ToIntegerOrInfinity(undefined)=0. Short-circuit to ConstI64(0)
        // before coerce_to_i64, which can't lower a ConstPtrNull undef
        // sentinel.
        let arg0_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
        let idx_val = if arg0_undef {
            Operand::ConstI64(0)
        } else {
            let idx_raw = ctx.lower_expr(args[0]);
            ctx.coerce_to_i64(idx_raw)
        };
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
        // S272 — eval-and-drop trailing exprs so side effects fire
        // per ES §22.1.3.2 trailing-arg ignore semantics.
        for &a in &args[1..] {
            let _ = ctx.lower_expr(a);
        }
        return Some(Operand::Value(v));
    }
    // P11.6-S4 — `s.normalize(form?)` per ES §22.1.3.13. Routes to
    // `__torajs_str_normalize(s, form)`. Default `form` is the
    // interned `"NFC"` literal (per ES step 2: form coerces to
    // "NFC" when undefined). The runtime intrinsic parses the form
    // string, raises `RangeError` via the cross-TU throw stub on
    // invalid forms, and otherwise returns a freshly-allocated Str
    // holding the normalized result. The per-call `emit_throw_check`
    // propagates the throw to the user's `try/catch` before the
    // result flows downstream.
    // S210 — `s.search()` / `s.search(undefined)` per ES §22.1.3.20:
    // RegExpCreate(undefined, undefined) yields an empty regex which
    // matches at index 0 in any string. Short-circuit before the
    // generic dispatch which routes search through str_index_of —
    // the indexOf path would default the missing arg to the literal
    // "undefined" and return -1 / first occurrence of "undefined",
    // not the spec'd 0.
    if recv_ty == Type::Str
        && method == "search"
        && (args.is_empty()
            || (args.len() == 1
                && matches!(
                    ctx.expr_types.get(&args[0]),
                    Some(crate::check::Type::Undefined)
                )))
    {
        return Some(Operand::ConstI64(0));
    }
    if recv_ty == Type::Str && method == "normalize" {
        // S208 — explicit `undefined` form follows the same
        // default-undefined rule per spec §22.1.3.13 step 1:
        // when `form` is undefined, default to "NFC". Detect
        // the typed-Undefined arg (same idiom S206/S207 use)
        // and route to the 0-arg path so the operand lower is
        // skipped (the literal has no side effects).
        // S240 widens the 1-arg detection to 2-arg so a trailing-undef
        // shape (normalize(undef, trailing)) still folds to "NFC" without
        // lowering the undef operand into the helper's Str slot.
        //
        // S272 — widen further to `args.len() >= 1` (any trailing count);
        // detect typed-Undefined at args[0] regardless of arg count, then
        // eval-and-drop args[1..] so trailing side-effect exprs fire.
        let undef_form = !args.is_empty()
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined)
            );
        let form_op = if args.is_empty() || undef_form {
            Operand::Value(ctx.intern_string_literal("NFC"))
        } else {
            ctx.lower_expr(args[0])
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_normalize, vec![recv_op, form_op]),
            Type::Str,
            None,
        );
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }
    // ES2024 §22.1.3.10 / §22.1.3.30 — `isWellFormed()` / `toWellFormed()`.
    // torajs strings are internally UTF-8 so lone surrogates can't be
    // encoded at all: every reachable Str is well-formed by construction.
    // `isWellFormed` returns true; `toWellFormed` is the identity. Both
    // arms drop the receiver dependency at the caller per existing
    // method-call ownership rules.
    if recv_ty == Type::Str && method == "isWellFormed" {
        return Some(Operand::ConstBool(true));
    }
    if recv_ty == Type::Str && method == "toWellFormed" {
        ctx.emit_rc_inc(recv_op.clone());
        return Some(recv_op);
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
                | "at"
                | "lastIndexOf"
                | "localeCompare"
                | "search"
        )
    {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(recv_op);
        // S140 — locale-variant case methods drop any locales arg; the
        // runtime helper is 1-arg only (en-US default, see check.rs).
        let drop_args = matches!(method.as_str(), "toLocaleLowerCase" | "toLocaleUpperCase");
        if !drop_args {
            // S207 — for replace / replaceAll the helper requires
            // Str operands; an explicit-undefined arg's lower path
            // would emit a non-Str value and SEGV the helper.
            // Replace the operand inline with the interned
            // "undefined" literal — same idiom S206 uses for
            // Array.join's undefined sep.
            //
            // S209 — for repeat the count arg is a number; per
            // spec §22.1.3.17 step 1 ToIntegerOrInfinity(undefined)
            // = 0, so an explicit-undefined arg lowers to
            // ConstI64(0) and the helper returns "".
            // S211 — localeCompare(undefined) per §22.1.3.10 step 4
            // takes the same ToString(undefined) = "undefined" path.
            let undef_to_str_repl =
                matches!(method.as_str(), "replace" | "replaceAll" | "localeCompare");
            let undef_to_zero = method.as_str() == "repeat";
            // S235 — String.{indexOf,lastIndexOf,includes,startsWith,endsWith,
            // search}(undefined) per ES §22.1.3.{8,10,5,21,7,16} step 1-3:
            // ToString(undefined) = "undefined". For the search-string slot
            // only (arg 0); the fromIndex/position slot keeps its
            // numeric undef carve-out (S214 / S216 / S224). Replace the
            // undef operand with the interned "undefined" literal so
            // the helper's (Str, Str) ABI receives a concrete pointer.
            let undef_to_str_at_arg0 = matches!(
                method.as_str(),
                "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" | "search"
            );
            // S214 — String.indexOf(needle, undefined) per ES
            // §22.1.3.8 step 4: ToIntegerOrInfinity(undefined)=0,
            // so fromIndex=undefined lowers to ConstI64(0). Only
            // applies to args[1] (fromIndex); args[0] (needle)
            // keeps generic lower (0-arg-undef-needle covered by
            // §22.1.3.8 "undefined" literal default block below).
            //
            // S224 — extend the same zero-default to startsWith /
            // includes per ES §22.1.3.{21,5}:
            // `ToIntegerOrInfinity(undefined)=0`, so position=undef
            // matches "search from index 0". (endsWith uses the
            // length-default branch — see undef_max_at_arg1 below.)
            let undef_zero_at_arg1 =
                matches!(method.as_str(), "indexOf" | "startsWith" | "includes") && args.len() == 2;
            // S215 — String.split(sep, undefined) per ES §22.1.3.21
            // step 2: limit undefined → 2^32-1 (effectively no
            // truncation). Lower limit=undefined to ConstI64(i64::MAX);
            // the take-min branch downstream Slt(MAX, len) is false →
            // take_slot = len, matching spec "no truncation".
            //
            // S216 — String.lastIndexOf(needle, undefined) per ES
            // §22.1.3.10 step 5: ToNumber(undefined)=NaN, NaN→pos=+∞.
            // Effective fromIndex=length; lower to ConstI64(i64::MAX)
            // and the str_last_index_of_from helper clamps `from > len`
            // to len (see torajs-str lookup.rs::last_index_of_from).
            //
            // S224 — extend the same length-default to endsWith per
            // ES §22.1.3.7: endPosition=undef → len(O). The
            // `str_ends_with_from` helper clamps `end > s.len()`
            // back to `s.len()` (lookup.rs::ends_with_from).
            let undef_max_at_arg1 =
                matches!(method.as_str(), "split" | "lastIndexOf" | "endsWith") && args.len() == 2;
            // S221 — String.substring(undefined [, undefined]) per ES
            // §22.1.3.22 step 4/5: start=undef → 0, end=undef → length.
            // Replace each undef slot before lower so the str_substring
            // helper's (Str, I64, I64) ABI never sees an Undefined
            // operand. recv.length is emitted lazily — only if arg[1]
            // is actually undef.
            //
            // S232 — extend the same undef-slot substitution to `slice`
            // 2-arg per ES §22.1.3.20 step 3/4: start undef → 0, end
            // undef → length. The 1-arg-undef shape for slice/substring
            // is handled by the existing V3-18 m1.h.36 fallthrough
            // below — once the loop substitutes the lone undef arg
            // with ConstI64(0), the `args.len() < 2` path appends the
            // recv length, yielding `[0, len]`.
            // S241 widens the 2-arg detection to 3-arg so a trailing-arg
            // shape (slice/substring(a, b, trailing)) still substitutes
            // undef slots; the trailing operand is dropped by the loop
            // break-at-i>1 guard below.
            let slice_subs_2arg = matches!(method.as_str(), "substring" | "slice")
                && (args.len() == 2 || args.len() == 3);
            let mut substring_len_op: Option<Operand> = None;
            let slice_subs_1arg_undef =
                matches!(method.as_str(), "substring" | "slice") && args.len() == 1;
            // S222 — String.{at,charAt,charCodeAt,codePointAt}(undefined)
            // per ES §22.1.3.{1,2,3,4} step 2-3:
            // ToIntegerOrInfinity(undefined)=0. Replace the single undef
            // index slot with ConstI64(0) — equivalent to the 0-arg
            // shape these methods already accept.
            let undef_zero_at_arg0_idx = matches!(
                method.as_str(),
                "at" | "charAt" | "charCodeAt" | "codePointAt"
            ) && args.len() == 1;
            // S223 — String.{padStart,padEnd}(undefined [, fillStr]) per
            // ES §22.1.3.{16,17} step 1: ToLength(undefined)=0, and step
            // 2 short-circuits because `0 <= S.length`, so the helper
            // returns S unchanged. Replace the undef maxLength slot with
            // ConstI64(0); the V3-18 1-arg fallthrough below still
            // supplies the default fill " " when arg 1 is omitted.
            let undef_zero_at_arg0_pad =
                matches!(method.as_str(), "padStart" | "padEnd") && (1..=3).contains(&args.len());
            // S236 — String.{padStart,padEnd}(N, undefined) per ES
            // §22.1.3.{16,17} step 6.a: fillString undef → " ". Replace
            // the undef fillStr slot with the interned " " literal so
            // the helper's (Str, I64, Str) ABI never sees a ConstPtrNull;
            // matches the V3-18 m1.h.45 1-arg default fill.
            //
            // S241 widens the 2-arg detection to 3-arg so a padStart/padEnd
            // (N, undef, trailing) shape still substitutes the " " literal;
            // the trailing operand is dropped by the loop break-at-i>1
            // guard below.
            let undef_space_at_arg1_pad = matches!(method.as_str(), "padStart" | "padEnd")
                && (args.len() == 2 || args.len() == 3);
            for (i, &a) in args.iter().enumerate() {
                let arg_undef =
                    matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined));
                // S238 — String.localeCompare(other, locales?, options?) per
                // ES §22.1.3.10 trailing-arg ignore: tora's bytewise helper
                // has no Intl-locale awareness, so the trailing slots are
                // dropped at lower-time. The helper signature stays
                // (Str, Str) and never receives the extra operands.
                if method.as_str() == "localeCompare" && i > 0 {
                    break;
                }
                // S239 — String.{indexOf,lastIndexOf,includes,startsWith,
                // endsWith}(needle, fromIndex, ...trailing) trailing-arg
                // ignore per ES §22.1.3.{8,10,5,21,7}. Drop args beyond
                // i=1 so the _from helper's (Str, Str, I64) ABI never
                // sees the extra operands.
                if matches!(
                    method.as_str(),
                    "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
                ) && i > 1
                {
                    break;
                }
                // S240 — String.{at,charAt,charCodeAt,codePointAt,
                // repeat,normalize}(useful, ...trailing) trailing-arg
                // ignore per ES §22.1.3.{1,2,3,4,17,13}: drop args
                // beyond i=0 so the helper's 1-arg ABI never sees the
                // extra operands.
                //
                // S272 — was `break` (silently dropped trailing exprs).
                // Lower-and-drop each so step()-style side-effect exprs
                // fire per ES eval-then-discard semantics. The operand
                // value is discarded (never pushed into argv).
                if matches!(
                    method.as_str(),
                    "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "normalize"
                ) && i > 0
                {
                    let _ = ctx.lower_expr(a);
                    continue;
                }
                // S241 — String.{slice,substring,substr,padStart,padEnd}
                // (a, b, ...trailing) trailing-arg ignore per ES
                // §22.1.3.{20,22,23,16,17}: drop args beyond i=1 so the
                // helper's 2-arg ABI never sees the extra operands.
                if matches!(
                    method.as_str(),
                    "slice" | "substring" | "substr" | "padStart" | "padEnd"
                ) && i > 1
                {
                    break;
                }
                if undef_to_str_repl && arg_undef {
                    let u = ctx.intern_string_literal("undefined");
                    argv.push(Operand::Value(u));
                } else if undef_to_str_at_arg0 && arg_undef && i == 0 {
                    let u = ctx.intern_string_literal("undefined");
                    argv.push(Operand::Value(u));
                } else if undef_to_zero && arg_undef {
                    argv.push(Operand::ConstI64(0));
                } else if undef_zero_at_arg1 && arg_undef && i == 1 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_max_at_arg1 && arg_undef && i == 1 {
                    argv.push(Operand::ConstI64(i64::MAX));
                } else if undef_zero_at_arg0_idx && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_zero_at_arg0_pad && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_space_at_arg1_pad && arg_undef && i == 1 {
                    let space = ctx.intern_string_literal(" ");
                    argv.push(Operand::Value(space));
                } else if slice_subs_2arg && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if slice_subs_2arg && arg_undef && i == 1 {
                    if substring_len_op.is_none() {
                        let len = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Load(Type::I64, recv_op, 8),
                            Type::I64,
                            None,
                        );
                        substring_len_op = Some(Operand::Value(len));
                    }
                    argv.push(substring_len_op.clone().unwrap());
                } else if slice_subs_1arg_undef && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else {
                    argv.push(ctx.lower_expr(a));
                }
            }
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
        //
        // S201 — 0-arg form: per ES §22.1.3.{16,17} step 1
        // `ToLength(undefined) = 0`, and step 2 returns S
        // unchanged because `0 <= S.length`. Push
        // (maxLen=0, fill=" ") so the helper takes the
        // no-pad path and returns a fresh clone of S.
        if matches!(method.as_str(), "padStart" | "padEnd") {
            if args.is_empty() {
                argv.push(Operand::ConstI64(0));
            }
            if args.len() <= 1 {
                let space = ctx.intern_string_literal(" ");
                argv.push(Operand::Value(space));
            }
        }
        // ES §22.1.3.1 step 2-3 — `s.at(index?)` undefined index
        // routes through ToIntegerOrInfinity → 0, so `s.at()`
        // returns `s[0]`. The `str_at` helper requires both
        // (str_ptr, i) args; push the default 0 when omitted.
        if method.as_str() == "at" && args.is_empty() {
            argv.push(Operand::ConstI64(0));
        }
        // ES §22.1.3.{8,13,14,21,22} — `searchString` defaults to
        // `undefined` when omitted; the algorithm reads it via
        // ToString → "undefined". For 0-arg `s.indexOf() /
        // includes() / lastIndexOf() / startsWith() / endsWith()`,
        // emit the literal "undefined" so the runtime helpers see a
        // valid Str needle. Matches bun (returns -1/false unless the
        // haystack contains "undefined").
        if matches!(
            method.as_str(),
            "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
        ) && args.is_empty()
        {
            let u = ctx.intern_string_literal("undefined");
            argv.push(Operand::Value(u));
        }
        // S207 — String.replace / replaceAll with fewer-than-2
        // args per ES §22.1.3.18 / §22.1.3.19 step 4 + step 6a:
        // missing slots default to undefined, ToString'd to the
        // literal "undefined". Helper expects both (searchString,
        // replaceValue) as Str. 0-arg → push "undefined" twice;
        // 1-arg → push "undefined" for the missing replaceValue.
        if matches!(method.as_str(), "replace" | "replaceAll") && args.len() < 2 {
            let u = ctx.intern_string_literal("undefined");
            if args.is_empty() {
                argv.push(Operand::Value(u));
            }
            argv.push(Operand::Value(u));
        }
        // V3-18 m1.h.50 — String.indexOf / lastIndexOf
        // with the 2-arg (needle, fromIndex) shape route
        // to the dedicated _from runtime helpers.
        if matches!(method.as_str(), "indexOf" | "lastIndexOf") && args.len() >= 2 {
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
        if matches!(method.as_str(), "startsWith" | "endsWith" | "includes") && args.len() >= 2 {
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
            "toUpperCase" | "toLocaleUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
            "toLowerCase" | "toLocaleLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
            "trim" => (ctx.intrinsics.str_trim, Type::Str),
            "trimStart" | "trimLeft" => (ctx.intrinsics.str_trim_start, Type::Str),
            "trimEnd" | "trimRight" => (ctx.intrinsics.str_trim_end, Type::Str),
            "padStart" => (ctx.intrinsics.str_pad_start, Type::Str),
            "padEnd" => (ctx.intrinsics.str_pad_end, Type::Str),
            "replace" => (ctx.intrinsics.str_replace, Type::Str),
            "replaceAll" => (ctx.intrinsics.str_replace_all, Type::Str),
            "at" => (ctx.intrinsics.str_at, Type::Str),
            // P11.3-A1 — `codePointAt` combines surrogate pairs
            // per ES §22.1.3.3; `charCodeAt` returns the lone
            // UTF-16 code unit per §22.1.3.2. Two distinct
            // intrinsics now that P11.1-S1/S5 gave us proper
            // hybrid Latin-1 / UTF-16 storage.
            "charCodeAt" => (ctx.intrinsics.str_char_code_at, Type::I64),
            "codePointAt" => (ctx.intrinsics.str_code_point_at, Type::I64),
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
                // ES §22.1.3.21 step 4 — `s.split()` (no sep) returns
                // `[S]`. Route to the dedicated 1-arg helper instead
                // of emitting a 1-arg `Call(str_split, [recv])` whose
                // missing `sep` slot reads register garbage and
                // SIGSEGV's once any prior `.split(arg)` shifted the
                // residual register state.
                // S233 — `s.split(undefined)` per ES §22.1.3.21 step 2:
                // separator===undefined skips the splitting altogether
                // and step 3 returns `[S]`. Without this carve-out the
                // 1-arg-undef path falls through to `str_split` with a
                // ConstPtrNull sep slot, which SIGSEGV's the helper's
                // (Str, Str) ABI. Reroute to the same 0-arg helper.
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
                    return Some(Operand::Value(v));
                }
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
                    // ES §22.1.3.21 step 6 — limit goes through
                    // ToUint32, which truncates toward 0 for finite
                    // numbers. Without this coerce a fractional / NaN /
                    // ±∞ literal stays f64 and the signed-int
                    // ICmp/Store below panics backend GPR
                    // materialization. coerce_to_i64 const-folds NaN→0
                    // / ±∞→i64::{MAX,MIN} (downstream clamp picks
                    // len), trunc for finite f64.
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
                    return Some(Operand::Value(v));
                }
                (ctx.intrinsics.str_split, Type::Arr(arr_id))
            }
            _ => unreachable!(),
        };
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
        // ES §22.1.3.16 step 4 — `s.repeat(n < 0)` throws RangeError.
        // Runtime sets a TLS pending throw; emit_throw_check propagates.
        if method == "repeat" {
            ctx.emit_throw_check(None);
        }
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
        // ES §22.1.3.32 step 5.b — toLocaleString routes
        // numeric elements through Number.toLocaleString
        // (en-US group separator). String / Substr / Bool
        // / Any keep the ToString-equivalent helpers
        // because their .toLocaleString returns the same
        // value (no Intl substrate engaged).
        let is_locale = method == "toLocaleString";
        let join_fid = match elem_ty {
            Type::Substr => ctx.intrinsics.arr_join_substr,
            Type::I64 => {
                if is_locale {
                    ctx.intrinsics.arr_join_i64_locale
                } else {
                    ctx.intrinsics.arr_join_i64
                }
            }
            Type::F64 => {
                if is_locale {
                    ctx.intrinsics.arr_join_f64_locale
                } else {
                    ctx.intrinsics.arr_join_f64
                }
            }
            Type::Bool => ctx.intrinsics.arr_join_bool,
            Type::Any => ctx.intrinsics.arr_join_any, // S126-4
            _ => ctx.intrinsics.arr_join,
        };
        let mut argv = Vec::with_capacity(2);
        argv.push(recv_op);
        // V3-18 m1.h.42 — default separator ","
        // when join() is called with no arg.
        //
        // S206 — explicit `undefined` sep follows the same
        // default-undefined rule per spec §23.1.3.16 step 1:
        // if sep is undefined → sep = ",". Detect the
        // typed-Undefined arg shape and skip the operand
        // lower (the literal has no side effects to drop).
        // S242 widens the 1-arg detection to 2-arg so the trailing-arg
        // shape `xs.join(undef, trailing)` still folds to "," without
        // lowering the undef operand into the helper's Str slot.
        let undef_sep = (args.len() == 1 || args.len() == 2)
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined)
            );
        let sep = if args.is_empty() || undef_sep {
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
        } else if let Expr::Ident(name) = ctx.ast.get_expr(args[0])
            && name == "Infinity"
        {
            // S129-5 `xs.flat(Infinity)` — ES §23.1.3.13 spec form
            // for full-depth flatten. ssa-lower unrolls flat-1
            // calls; the typed branch early-breaks once cur_ty
            // stops being Arr<Arr<T>>, and the Array<Any> branch's
            // arr_flat_any is a no-op shallow clone when no slot
            // wraps an inner Array<Any>. Using i64::MAX would
            // explode loop bookkeeping at lower-time — 64 unrolls
            // are plenty for realistic nesting (matches V8 / JSC
            // arbitrary limits in spec-test fixtures).
            64
        } else if let Expr::Ident(name) = ctx.ast.get_expr(args[0])
            && name == "undefined"
        {
            // S220 — `xs.flat(undefined)` per ES §23.1.3.10 step 1:
            // `If depth is undefined, depthNum = 1`. Equivalent to the
            // 0-arg `xs.flat()` default.
            1
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
            // S129-3 Array<Any>.flat — outer_elem is Type::Any
            // (NaN-box AnyValue per slot). Routes to the Any-aware
            // runtime helper which decodes each slot's tag and
            // extends inner Array<Any> via arr_extend_any (rc-safe)
            // / passes other slots through as a single push. Result
            // type stays Array<Any> (depth=N unrolls N flat-1 calls,
            // mirror of the typed branch below).
            if matches!(outer_elem, Type::Any) {
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_flat_any, vec![cur]),
                    cur_ty,
                    None,
                );
                cur = Operand::Value(v);
                continue;
            }
            let Type::Arr(_) = outer_elem else {
                // ES §23.1.3.11 — non-Array slot: receiver is already
                // "flat" at this depth, but flat() must still return a
                // fresh array. Emit a shallow clone (arr_slice + rc_inc
                // range on refcounted elem) — same shape as depth=0.
                let len = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::I64, cur, ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_slice,
                        vec![cur, Operand::ConstI64(0), Operand::Value(len)],
                    ),
                    cur_ty,
                    None,
                );
                if let Type::Arr(arr_id) = cur_ty {
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
                cur = Operand::Value(v);
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
    {
        // S276 — widen from `args.len() <= 1` to any arg count per ES
        // §23.1.3.{30,33} trailing-arg ignore. SSA-emit reads only
        // args[0] (cmp); trailing args eval-and-drop below so side-
        // effect exprs fire.
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
        //
        // ES §23.1.3.{29,31} step 1 also accepts `undefined` for
        // the comparator literal; route a 1-arg call with arg type
        // Undefined to the same default-compare path (check.rs
        // mirror at the sort/toSorted arity special-case).
        let (cmp_val, cmp_ty) = if let Some(arg0) = args.first() {
            let arg_static_ty = ctx.expr_types.get(arg0).cloned();
            if matches!(arg_static_ty, Some(crate::check::Type::Undefined)) {
                // Drop the operand without lowering its side
                // effects — `undefined` is a literal keyword with
                // none.
                (None, None)
            } else {
                let v = ctx.lower_expr(*arg0);
                let t = ctx.operand_ty(&v);
                (Some(v), Some(t))
            }
        } else {
            (None, None)
        };
        // S276 — eval-and-drop trailing args past cmp so step()-style
        // side-effect exprs fire per ES §23.1.3.{30,33} trailing-arg
        // ignore. SSA-emit reads only args[0]; args[1..] discarded.
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
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
            _ => {
                // ES §23.1.3.30 SortCompare with no comparator =
                // ToString each operand then code-unit lex compare.
                // Pre-Spec-fix wedge used numeric `prev > cur` which
                // ordered `[10, 2]` as `[2, 10]` — disagrees with bun.
                // For each numeric / bool element, route through the
                // existing `*_to_str` intrinsics so the resulting Str
                // operands feed the same `str_locale_compare`
                // (bytewise; see runtime doc) the Str arm uses. Obj /
                // Arr / etc fall back to the legacy pointer ICmp —
                // no `*_to_str` exists for them yet and the spec
                // result (`"[object Object]"`-tied tie-break) is
                // niche enough to leave behind a follow-up.
                let to_str = |ctx: &mut LowerCtx, v, ty: Type| match ty {
                    Type::Str | Type::Substr => Some(v),
                    Type::I64 => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.i64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    Type::F64 => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.f64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    Type::Bool => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.bool_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    _ => None,
                };
                let prev_s = to_str(ctx, prev, elem_ty);
                let cur_s = to_str(ctx, cur, elem_ty);
                if let (Some(ps), Some(cs)) = (prev_s, cur_s) {
                    let r = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.str_locale_compare,
                            vec![Operand::Value(ps), Operand::Value(cs)],
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
                } else {
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(prev), Operand::Value(cur)),
                        Type::Bool,
                        None,
                    )
                }
            }
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
        for &a in args {
            // S212 — explicit `undefined` arg per ES §22.1.3.4
            // step 3.a: each arg is ToString'd, undefined →
            // "undefined". Inline-substitute the interned
            // literal so the helper sees a valid Str pointer
            // — same idiom S207/S211 use for replace/locale-
            // Compare.
            let other = if matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined)) {
                let u = ctx.intern_string_literal("undefined");
                Operand::Value(u)
            } else {
                ctx.lower_expr(a)
            };
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
        let recv_elem = ctx.arr_layouts[arr_id.0 as usize];
        // ES §23.1.3.2 — concat returns a fresh array; receiver must
        // not be mutated. `arr_concat` always allocates a new buffer,
        // but `arr_push` (used below for scalar args) may mutate
        // in-place when capacity allows. Track when `acc` is still
        // aliased to the receiver and force a shallow copy before the
        // first scalar push to preserve spec semantics.
        let mut acc_is_fresh = args.is_empty();
        for a in args {
            let other = ctx.lower_expr(*a);
            let other_ty = ctx.operand_ty(&other);
            // ES §23.1.3.2 — scalar arg (same type as receiver elem)
            // is appended as a single element instead of spread.
            if other_ty == recv_elem && !matches!(other_ty, Type::Arr(_)) {
                if !acc_is_fresh {
                    let len = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(Type::I64, acc, ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    let v = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.arr_slice,
                            vec![acc, Operand::ConstI64(0), Operand::Value(len)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    );
                    acc = Operand::Value(v);
                    acc_is_fresh = true;
                }
                let push_arg = ctx.raw_slot_arg(other);
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_push, vec![acc, push_arg]),
                    Type::Arr(arr_id),
                    None,
                );
                acc = Operand::Value(v);
                continue;
            }
            // S129-4 Array<Any>.concat(Array<typed>) — receiver elem
            // is Any (NaN-box AnyValue per slot), arg is a typed
            // Array<T>. The classic arr_concat is a raw 8B-stride
            // memcpy which would copy T's raw bits straight into
            // Array<Any> slots — wrong (NaN-box expects tag/value
            // pairs). Route to arr_extend_typed_into_any with the
            // T-derived elem_tag so the runtime pairs each raw slot
            // with the right ANY_* tag before append. Heap T's
            // rc_inc is handled inside the helper. Same Array<Any>
            // mixed-typed escape series as S128-1..3 push / fill.
            let typed_into_any = matches!(recv_elem, Type::Any)
                && matches!(other_ty, Type::Arr(oid) if !matches!(ctx.arr_layouts[oid.0 as usize], Type::Any));
            let v = if typed_into_any {
                let Type::Arr(oid) = other_ty else {
                    unreachable!()
                };
                let oet = ctx.arr_layouts[oid.0 as usize];
                let elem_tag = match oet {
                    Type::Bool => 1,
                    Type::I64 | Type::I32 => 2,
                    Type::F64 => 3,
                    t if t.is_refcounted() => 4,
                    other => panic!(
                        "ssa-lower: Array<Any>.concat typed-arg elem {other:?} not supported"
                    ),
                };
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_extend_typed_into_any,
                        vec![acc, other, Operand::ConstI64(elem_tag)],
                    ),
                    Type::Arr(arr_id),
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_concat, vec![acc, other]),
                    Type::Arr(arr_id),
                    None,
                )
            };
            acc = Operand::Value(v);
            // arr_concat / arr_extend_typed_into_any both return new
            // ptrs — acc is now detached from the receiver buffer.
            acc_is_fresh = true;
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
    // ES §23.1.3.1: args[1] is never read (i_val uses args[0] only).
    if let Type::Arr(arr_id) = recv_ty
        && method == "at"
        && args.len() <= 2
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
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
    // S246 widens the 3-arg ceiling to 4-arg so the trailing-arg
    // shape (copyWithin(t, s, e, trailing)) still routes through
    // this fast path; args[3] is never lowered (trailing-arg ignore).
    if let Type::Arr(arr_id) = recv_ty
        && method == "copyWithin"
        && (1..=4).contains(&args.len())
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
        // S219 — Array.copyWithin(target [, start [, end ]]) per ES
        // §23.1.3.4: target/start go through ToIntegerOrInfinity
        // (undef → 0); end===undefined takes the omitted default (len).
        // Short-circuit each undef slot before relative_to_len so the
        // helper isn't called on a ConstPtrNull undef sentinel.
        let arg0_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
        let target = if arg0_undef {
            Operand::ConstI64(0)
        } else {
            let raw_target = ctx.lower_expr(args[0]);
            ctx.relative_to_len(raw_target, Operand::Value(len_for_norm))
        };
        let start = if args.len() >= 2 {
            let arg1_undef = matches!(
                ctx.expr_types.get(&args[1]),
                Some(crate::check::Type::Undefined)
            );
            if arg1_undef {
                Operand::ConstI64(0)
            } else {
                let raw = ctx.lower_expr(args[1]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() >= 3 {
            let arg2_undef = matches!(
                ctx.expr_types.get(&args[2]),
                Some(crate::check::Type::Undefined)
            );
            if arg2_undef {
                Operand::Value(len_for_norm)
            } else {
                let raw = ctx.lower_expr(args[2]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
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
        // ES §23.1.3.39 step 7 — out-of-range index throws
        // RangeError. The runtime helper records a pending throw via
        // TLS and returns a null ptr; propagate immediately so the
        // following rc-inc walk never touches the null result.
        ctx.emit_throw_check(None);
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
    // S246 widens the 3-arg ceiling to 4-arg so the trailing-arg
    // shape (fill(v, s, e, trailing)) still routes through this
    // fast path; args[3] is never lowered (trailing-arg ignore).
    if let Type::Arr(arr_id) = recv_ty
        && method == "fill"
        && (args.len() >= 1 && args.len() <= 4)
    {
        let fill_elem = ctx.arr_layouts[arr_id.0 as usize];
        // L3b Array<Any>.fill — NaN-box the value and route through
        // `__torajs_arr_fill_any`. Pre-fix the non-Copy per-slot
        // loop below wrote raw bits into the 8-byte AnyValue slot
        // (StoreDyn(value, off) with value still raw), which
        // SIGSEGV'd the next decode pass. Mirror of the
        // local-Ident / K.8 / (b) Array<Any>.push helper family.
        if matches!(fill_elem, Type::Any) {
            return Some(ctx.emit_arr_any_fill_at(recv_op, args, recv_ty));
        }
        let mut value = ctx.lower_expr(args[0]);
        // W4 — align with the elem width, then cross the i64-param
        // intrinsic boundary as raw bits.
        if fill_elem == Type::F64 && ctx.operand_ty(&value) == Type::I64 {
            value = ctx.coerce_to_f64(value);
        }
        let value = ctx.raw_slot_arg(value);
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
        // S218 — Array.fill(v, undefined [, undefined]) per ES
        // §23.1.3.7 step 5/9: start=undef → 0 (omitted default),
        // end=undef → len (omitted default). Short-circuit each undef
        // slot before lower so relative_to_len isn't invoked on a
        // ConstPtrNull/I64 undef sentinel.
        let start = if args.len() >= 2 {
            let arg1_undef = matches!(
                ctx.expr_types.get(&args[1]),
                Some(crate::check::Type::Undefined)
            );
            if arg1_undef {
                Operand::ConstI64(0)
            } else {
                let raw = ctx.lower_expr(args[1]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() >= 3 {
            let arg2_undef = matches!(
                ctx.expr_types.get(&args[2]),
                Some(crate::check::Type::Undefined)
            );
            if arg2_undef {
                Operand::Value(len_for_norm)
            } else {
                let raw = ctx.lower_expr(args[2]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
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
        ctx.emit_rc_inc(value);
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
    // S242 — Array<T>.slice(start, end, ...trailing) trailing-arg
    // ignore per ES §23.1.3.28: args[2] is never read; the end
    // branch below switches on `args.len() >= 2 && !arg1_undef`.
    if let Type::Arr(arr_id) = recv_ty
        && method == "slice"
        && args.len() <= 3
    {
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
        let start = if args.is_empty() || arg0_undef {
            Operand::ConstI64(0)
        } else {
            ctx.lower_expr(args[0])
        };
        argv.push(start);
        let end = if args.len() >= 2 && !arg1_undef {
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
        && (1..=3).contains(&args.len())
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
            let raw = if arg1_undef {
                Operand::ConstI64(0)
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
