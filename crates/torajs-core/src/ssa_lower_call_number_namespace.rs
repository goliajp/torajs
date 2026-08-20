//! `Number.<method>(args)` namespace-static dispatch — `parseInt`,
//! `parseFloat`, `isInteger`, `isNaN`, `isFinite`, `isSafeInteger` —
//! pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-8 of the `Expr::Call` god-arm decomp (chunks 1-7 =
//! Arr higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods).
//!
//! Three method groups:
//! - `parseInt(s, radix?)` — alias of global parseInt per §21.1.2.13
//!   (S327 widens radix to Any via any_to_number → coerce_to_i64).
//! - `parseFloat(s)` — alias of global parseFloat per §21.1.2.12
//!   (S330 widens to Any via the `decode_any_to_str` shape).
//! - `is{Integer|NaN|Finite|SafeInteger}` — strict-Type predicate per
//!   §21.1.2.{3,5,7}: non-Number args statically return false; Any path
//!   goes through `num_is_*_any` (S341).
//!
//! `Number.<m>` is a namespace member — `Expr::Member { obj: Ident("Number"),
//! name: m }`. Not the same shape as `<recv>.toFixed(...)` (chunk 5 in
//! `ssa_lower_call_number_methods.rs`).
//!
//! Returns `Some(result)` when the callee matches the namespace shape with
//! one of the 6 method names; `None` lets the caller fall through to the
//! next arm (`BigInt.asIntN` / `BigInt.asUintN` directly below).

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `Number.<method>(args)`. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(ctx.ast.get_expr(ns_id), Expr::Ident(n) if n == "Number") {
        return None;
    }
    let result = match m_name.as_str() {
        "parseInt" => lower_parse_int(ctx, args),
        "parseFloat" => lower_parse_float(ctx, args),
        "isInteger" | "isNaN" | "isFinite" | "isSafeInteger" => {
            lower_is_predicate(ctx, m_name.as_str(), args)
        }
        other => panic!("ssa-lower: unknown Number method `{other}`"),
    };
    Some(result)
}

fn lower_parse_int(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // S202 — Number.parseInt is a §21.1.2.13 alias to global parseInt;
    // 0-arg form yields NaN by the same default-undefined path.
    // S226 — explicit-undefined arg folds the same way without lowering
    // the arg.
    // S302 — lower-and-drop trailing args[2..] per S272 idiom so step()-
    // style side-effect exprs fire per ES eval-then-discard (check.rs
    // S253 already typecheck-dropped).
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        return Operand::ConstF64(f64::NAN);
    }
    // Number.parseInt(s, radix) — radix optional in JS; typecheck enforces
    // 2-arg shape so we always have a ConstI64 / loaded radix here.
    let s = ctx.lower_expr(args[0]);
    // V3-18 m1.h.25 — auto-detect when no radix.
    //
    // S234 — accept explicit-undefined radix per §21.1.2.13 aliasing
    // §19.2.5.1 step 2-3: ToInt32(undefined)=0 → same auto-detect
    // sentinel; skip lowering the undef arg.
    let radix_undef = args.len() >= 2
        && matches!(
            ctx.expr_types.get(&args[1]),
            Some(check_mod::Type::Undefined)
        );
    let r = if args.len() >= 2 && !radix_undef {
        ctx.lower_expr(args[1])
    } else {
        Operand::ConstI64(0)
    };
    // Subset constraint: radix must be an integer-shaped expression
    // (literal or i64 binding) so no FpToSi is needed. Pass user-typed f64
    // through unchecked is a known v0 hole; doc'd in the test port.
    //
    // S327 — accept Any radix per check.rs widen: ES §19.2.5.1 step 2
    // ToInt32 already covers arbitrary-typed input. Route Any through
    // any_to_number → coerce_to_i64 instead of panicking. I64/I32/ConstI64
    // fast paths preserved as-is; F64 / Any go through coerce_to_i64
    // (NaN→0, ±∞→sat per spec).
    let r_ty = ctx.operand_ty(&r);
    let r = if r_ty == Type::I64 || r_ty == Type::I32 || matches!(r, Operand::ConstI64(_)) {
        r
    } else if r_ty == Type::Any {
        let f = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![r]),
            Type::F64,
            None,
        );
        // §21.1.2.13 forwards to §19.2.5, whose step 2 ToInt32(radix)
        // can throw — same check the bare-name sibling now runs.
        ctx.emit_throw_check(None);
        ctx.coerce_to_i64(Operand::Value(f))
    } else {
        ctx.coerce_to_i64(r)
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.num_parse_int, vec![s, r]),
        Type::F64,
        None,
    );
    Operand::Value(v)
}

fn lower_parse_float(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // S202 — Number.parseFloat is a §21.1.2.12 alias to global parseFloat;
    // 0-arg → NaN.
    // S226 — explicit-undefined arg folds the same way without lowering
    // the arg.
    // S302 — lower-and-drop trailing args[1..] per S272 idiom (check.rs
    // S253 already typecheck-dropped).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        return Operand::ConstF64(f64::NAN);
    }
    // S330 — accept Any per check.rs widen: decode Any via anyv_to_str_pair
    // (tag + value) so the helper's (Str) -> F64 ABI receives a concrete Str.
    let raw = ctx.lower_expr(args[0]);
    let raw_ty = ctx.operand_ty(&raw);
    let s = if raw_ty == Type::Any {
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![raw.clone()]),
            Type::I64,
            None,
        );
        let val = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value, vec![raw.clone()]),
            Type::I64,
            None,
        );
        let s_val = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_to_str,
                vec![Operand::Value(tag), Operand::Value(val)],
            ),
            Type::Str,
            None,
        );
        // any_to_str only borrowed the pair — reclaim a ShortStr-
        // materialized temp (no-op otherwise).
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_unbox_settle,
                vec![raw, Operand::Value(val)],
            ),
        );
        // Pending-throw propagation — see the bare-globals twin
        // (0-check audit, rotation 130 L3b).
        ctx.emit_throw_check(None);
        Operand::Value(s_val)
    } else {
        raw
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.num_parse_float, vec![s]),
        Type::F64,
        None,
    );
    Operand::Value(v)
}

fn lower_is_predicate(ctx: &mut LowerCtx<'_>, m_name: &str, args: &[ExprId]) -> Operand {
    // S202 — Number.is{Integer,NaN,Finite,SafeInteger} per §21.1.2.{3,5,7}
    // are strict (no ToNumber). Non-Number args statically return false;
    // the implicit undefined of a 0-arg call is the canonical non-Number
    // case.
    // S305 — lower-and-drop trailing args[1..] per S272 idiom so step()-
    // style side-effect exprs fire (check.rs S253 already typecheck-dropped;
    // SSA-emit short-circuits non-Number args + dispatches helper for
    // Number args, both reading only args[0]).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    if args.is_empty() {
        return Operand::ConstBool(false);
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // S341 — Any-typed arg: tag-dispatch via the `*_any` helper family.
    // Pre-fix the !matches! short-circuit below dropped Any to a static
    // false, silent-wrong vs spec when the Any boxed a real Number (e.g.
    // `const o: any = 2; Number.isInteger(o)` → bun: true, tr: false).
    // The helper inspects the (tag, val) pair — tag ∈ {ANY_I64=2,
    // ANY_F64=3} dispatches the existing _i/_f impl, every other tag
    // returns false per spec strict-Type-check.
    if matches!(arg_ty, Type::Any) {
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![arg_op.clone()]),
            Type::I64,
            None,
        );
        let val = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value, vec![arg_op.clone()]),
            Type::I64,
            None,
        );
        if ctx.expr_is_fresh_owned(args[0]) {
            ctx.emit_drop_value(arg_op.clone(), Type::Any);
        }
        let target = match m_name {
            "isInteger" => ctx.intrinsics.num_is_integer_any,
            "isNaN" => ctx.intrinsics.num_is_nan_any,
            "isFinite" => ctx.intrinsics.num_is_finite_any,
            "isSafeInteger" => ctx.intrinsics.num_is_safe_integer_any,
            _ => unreachable!(),
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, vec![Operand::Value(tag), Operand::Value(val)]),
            Type::Bool,
            None,
        );
        // the helper only borrowed the pair — reclaim a ShortStr-
        // materialized temp (no-op otherwise; arg_op's drop above
        // was a no-op for the ShortStr immediate so the bit
        // pattern is still a valid tag probe here).
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_unbox_settle,
                vec![arg_op, Operand::Value(val)],
            ),
        );
        return Operand::Value(v);
    }
    // V3-18 wedge — Number.is{Finite,NaN,Integer,SafeInteger} per spec do
    // NOT coerce: non-Number args return false statically. Drop the borrow
    // if the arg is refcounted (string / object / array) so the const-false
    // return doesn't leak.
    if !matches!(arg_ty, Type::I64 | Type::F64 | Type::I32) {
        if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
            ctx.emit_drop_value(arg_op, arg_ty);
        }
        return Operand::ConstBool(false);
    }
    let target = match (m_name, arg_ty) {
        ("isInteger", Type::F64) => ctx.intrinsics.num_is_integer_f,
        ("isInteger", _) => ctx.intrinsics.num_is_integer_i,
        ("isNaN", Type::F64) => ctx.intrinsics.num_is_nan_f,
        ("isNaN", _) => ctx.intrinsics.num_is_nan_i,
        ("isFinite", Type::F64) => ctx.intrinsics.num_is_finite_f,
        ("isFinite", _) => ctx.intrinsics.num_is_finite_i,
        ("isSafeInteger", Type::F64) => ctx.intrinsics.num_is_safe_integer_f,
        ("isSafeInteger", _) => ctx.intrinsics.num_is_safe_integer_i,
        _ => unreachable!(),
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(target, vec![arg_op]),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}
