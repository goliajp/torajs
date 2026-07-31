//! v0.2 #1 Phase 1b — `<Str>.{replace|replaceAll|split|match|matchAll}`
//! regex-receiver intercept pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-7
//! of the `Expr::Call` god-arm decomp (chunks 1-6 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number methods + bare-name
//! globals).
//!
//! Six methods (search joined at chunk 800) share
//! `Expr::Member { obj, name }` + first-arg-is-RegExp
//! peek; when both gates pass, dispatch routes to `__torajs_regex_*`
//! intrinsics. The non-regex `(Str, Str)` path is owned by
//! `ssa_lower_str::try_lower_method_call` (the M6.1 sidekick) downstream
//! — this block intercepts only when the first arg is statically a
//! regex (literal `/.../flags` or Ident with tracked Type::RegExp).
//! Substr receivers materialize through `substr_to_owned` before the
//! runtime call (chunk 800 — the byte reader misreads view blocks).
//!
//! Returns `Some(result)` when the regex intercept dispatches; `None`
//! lets the caller fall through to the M6.1 String/Substr/Array stdlib
//! sidekick + the subsequent Arr predicate-iterator family.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_call_str_regex_emit::{
    emit_match, emit_match_all, emit_replace, emit_search, emit_split,
};

/// Try to lower `<Str>.{replace|replaceAll|split|match|matchAll}` when the
/// first arg is a RegExp. Returns `Some` when dispatched, `None` otherwise.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        name.as_str(),
        "replace" | "replaceAll" | "split" | "match" | "matchAll" | "search"
    ) {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // Detection: peek the AST without lowering to avoid double side-effects
    // (re-evaluating the receiver if we were to fall through). Recognized
    // regex args:
    //   - Expr::Regex { ... } — literal `/.../flags`
    //   - Expr::Ident(name) — a local whose tracked SSA type is Type::RegExp
    // Anything else (incl. computed RegExp from a function call) falls
    // through to the existing string path, which currently rejects RegExp
    // args via Type::Any signature. A v0.2 #1.c follow-up can broaden the
    // detection — for now the literal + ident forms cover the dominant
    // idioms and all the test262 cases at hand.
    let arg0_is_regex = match ctx.ast.get_expr(args[0]) {
        Expr::Regex { .. } => true,
        Expr::Ident(n) => ctx
            .locals
            .get(n)
            .map(|info| info.ty == Type::RegExp)
            .unwrap_or(false),
        _ => false,
    };
    // RFC 20260716 刀 9 — `match` / `matchAll` accept a non-RegExp
    // arg via ES §22.1.3.{11,12} step 4.c: `RegExpCreate(ToString(P),
    // flags)`. match uses `""` flags; matchAll uses `"g"` (matchAll
    // kernel throws TypeError on non-global — the coerced RegExp
    // must be global implicitly). Emit the coerce inline below and
    // fall through to the shared dispatch block.
    //
    // Rotation 262 — `search` joins the coerce lane for an
    // Any-typed single arg only (§22.1.3.20 step 4; regex, not
    // indexOf, semantics): the string-arg spelling keeps its
    // member-table indexOf boundary and every other shape keeps
    // today's route.
    let arg0_is_any = matches!(ctx.expr_types.get(&args[0]), Some(crate::check::Type::Any));
    let coerce_match_lane = !arg0_is_regex
        && (matches!(name.as_str(), "match" | "matchAll")
            || (name == "search" && arg0_is_any && args.len() == 1));
    // Rotation 262 — `replace` with an Any-typed searchValue and
    // store evidence rides the §22.1.3.19 step-3 `@@replace` branch.
    // The replaceValue must be a shape BOTH lanes can emit (checker
    // String or Function — the fallback's literal-substring kernels);
    // everything else keeps today's route.
    let replace_symbol_lane = name == "replace"
        && !arg0_is_regex
        && arg0_is_any
        && (args.len() == 1
            || (args.len() == 2
                && matches!(
                    ctx.expr_types.get(&args[1]),
                    Some(crate::check::Type::String) | Some(crate::check::Type::Function(..))
                )))
        && crate::check_type_of_call_string_match::any_pattern_may_carry_matcher(ctx.ast, args[0]);
    if !arg0_is_regex && !coerce_match_lane && !replace_symbol_lane {
        return None;
    }
    let raw_recv = ctx.lower_expr(obj);
    // Chunk 800 — a Substr receiver (charAt view, for-of-str char,
    // exec capture) is a 16-byte parent-pointer block the runtime
    // `str_slice` reader would misread as an owned Str header
    // (probe: `ch.match(/o/)` answered null on a matching char,
    // `ch.split(/x/)` answered mojibake). Materialize through
    // `substr_to_owned` — the chunk-699 test/exec haystack dance —
    // and drop the fresh temp after the call. Owned-Str receivers
    // pass through.
    let recv_is_view = ctx.operand_ty(&raw_recv) == Type::Substr;
    let recv_op = if recv_is_view {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![raw_recv]),
            Type::Str,
            None,
        );
        Operand::Value(owned)
    } else {
        raw_recv
    };
    // §22.1.3.{13,20} step 3 — an Any-typed pattern may carry a
    // user `@@match` / `@@search` method; branch on the runtime
    // probe before the step-4 coerce. The checker mirrors this
    // exact gate with a `Type::Any` result
    // (`check_type_of_call_string_match` / `_search_regex`), so
    // only the single-arg shape with a checker-Any pattern AND
    // store evidence routes here; a store-free Any pattern falls
    // through to the plain coerce lane below.
    if replace_symbol_lane {
        let result = crate::ssa_lower_call_str_match_custom::lower_replace_any_pattern(
            ctx,
            recv_op.clone(),
            args,
        );
        if recv_is_view {
            ctx.emit_drop_value(recv_op, Type::Str);
        }
        return Some(result);
    }
    if coerce_match_lane
        && matches!(name.as_str(), "match" | "search")
        && arg0_is_any
        && crate::check_type_of_call_string_match::any_pattern_may_carry_matcher(ctx.ast, args[0])
    {
        use crate::ssa_lower_call_str_match_custom::SymbolDispatchKind;
        let kind = if name == "search" {
            SymbolDispatchKind::Search
        } else {
            SymbolDispatchKind::Match
        };
        let result = crate::ssa_lower_call_str_match_custom::lower_symbol_dispatch_pattern(
            ctx,
            recv_op.clone(),
            args,
            kind,
        );
        if recv_is_view {
            ctx.emit_drop_value(recv_op, Type::Str);
        }
        return Some(result);
    }
    let (re_op, minted_regex) = if coerce_match_lane {
        // ES §22.1.3.{11,12} step 4.c → `RegExpCreate(regexp, F)`.
        // RegExpCreate → RegExpInitialize (§22.2.3.2):
        //  - if pattern is undefined, P = "" (skip ToString)
        //  - else P = ToString(pattern)
        // Emit `""` directly for the undef case; otherwise route
        // through the shared `emit_to_string` (which handles
        // Str/I64/F64/Bool/Ptr(Null)/Any/Arr/Obj — the widest
        // spec-covered set). matchAll gets `"g"` flag per step 4.c.
        let raw_arg = ctx.lower_expr(args[0]);
        let arg_ty = ctx.operand_ty(&raw_arg);
        let arg_is_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        ) && matches!(raw_arg, Operand::ConstPtrNull);
        let pat_op = if arg_is_undef {
            // Drop the ConstPtrNull; it's a no-op operand but
            // release_owned_temp is defensive across arg shapes.
            let empty_lit = ctx.intern_string_literal("");
            Operand::Value(empty_lit)
        } else {
            crate::ssa_lower_call_coercion::emit_to_string(ctx, args[0], raw_arg, arg_ty, false)
        };
        let flags_bytes = if name == "matchAll" { "g" } else { "" };
        let flags_v = ctx.intern_string_literal(flags_bytes);
        let re_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.regex_compile,
                vec![pat_op.clone(), Operand::Value(flags_v)],
            ),
            Type::RegExp,
            None,
        );
        // Drop the coerced Str temp for non-undef path. The undef
        // shortcut passed an interned static-lifetime Str literal
        // (`intern_string_literal`) which the runtime does not
        // rc-tracked — no drop needed. For every non-Str arg type,
        // `emit_to_string` returns a fresh owned Str; the Str-arg
        // path emits rc_inc via the identity arm so unconditional
        // drop is correct on that fresh/borrowed ref.
        if !arg_is_undef {
            ctx.emit_drop_value(pat_op, Type::Str);
        }
        (Operand::Value(re_v), true)
    } else {
        (ctx.lower_expr(args[0]), false)
    };
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
    let result = match name.as_str() {
        "match" => emit_match(ctx, recv_op.clone(), re_op.clone(), args, arr_id),
        "matchAll" => emit_match_all(ctx, recv_op.clone(), re_op.clone(), args, arr_id),
        "split" => emit_split(ctx, recv_op.clone(), re_op, args, arr_id),
        "replace" | "replaceAll" => emit_replace(ctx, recv_op.clone(), re_op, &name, args),
        "search" => emit_search(ctx, recv_op.clone(), re_op, args),
        _ => unreachable!(),
    };
    if minted_regex {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.regex_drop, vec![re_op]),
        );
    }
    if recv_is_view {
        ctx.emit_drop_value(recv_op, Type::Str);
    }
    Some(result)
}
