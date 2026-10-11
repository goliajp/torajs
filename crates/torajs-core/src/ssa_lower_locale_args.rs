//! The locales / options arguments of the Number / BigInt / Date
//! `toLocale*String` calls: lowered, boxed and handed to the runtime
//! (`torajs-anyvalue` `locale_args`), which runs ECMA-402
//! CanonicalizeLocaleList and the constructor's option reads. Every
//! structurally valid tag is accepted (the en-US formatter is the
//! LookupMatcher answer for all of them).
//!
//! A string-literal `locales` is judged here instead: a valid one needs
//! no runtime look (and with no `options`, no call at all); an invalid
//! one is the §9.2.1 step 7.c.vi RangeError, emitted directly on the
//! Number / BigInt path. A Date call still asks the runtime, since a
//! NaN time value answers "Invalid Date" before the locales are read.
//! Array and TypedArray `toLocaleString` never come here: they invoke
//! each element's `toLocaleString` with no arguments (§23.1.3.32).

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// A `toLocaleString`-family call's `locales` / `options`, boxed
/// (`options` is undefined when absent), with the expressions they
/// came from for the release after the consuming call.
struct LocaleArgs {
    locales: Operand,
    options: Operand,
    srcs: [Option<(ExprId, Operand)>; 2],
}

fn box_undefined(ctx: &mut LowerCtx<'_>) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BoxAny(5, Operand::ConstI64(0)),
        Type::Any,
        None,
    );
    Operand::Value(v)
}

/// Evaluate every argument left to right (ECMA-402 reads only the
/// first two; the rest are evaluated and dropped). `None` for a call
/// without arguments.
fn lower_locale_args(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<LocaleArgs> {
    let mut srcs = [None, None];
    for (i, &a) in args.iter().enumerate() {
        let op = ctx.lower_expr(a);
        if i < 2 {
            srcs[i] = Some((a, ctx.box_to_any_from_expr(a, op)));
        }
    }
    let (_, locales) = srcs[0]?;
    let options = match srcs[1] {
        Some((_, op)) => op,
        None => box_undefined(ctx),
    };
    Some(LocaleArgs {
        locales,
        options,
        srcs,
    })
}

/// Release the boxed arguments once the consuming call is emitted.
fn release_locale_args(ctx: &mut LowerCtx<'_>, la: &LocaleArgs) {
    for (eid, op) in la.srcs.iter().flatten() {
        ctx.release_owned_temp(*eid, op);
    }
}

/// The `LOCALE_FORMAT_DATE_*` kind (`torajs-anyvalue` `locale_args`)
/// of a Date `toLocale*String` method.
pub(crate) fn date_locale_kind(method: &str) -> Option<i64> {
    match method {
        "toLocaleString" => Some(1),
        "toLocaleDateString" => Some(2),
        "toLocaleTimeString" => Some(3),
        _ => None,
    }
}

/// A string-literal tag and whether it is structurally valid (§6.2.1).
fn literal_tag(ctx: &LowerCtx<'_>, e: ExprId) -> Option<(torajs_wtf8::Wtf8Buf, bool)> {
    match ctx.ast.get_expr(e) {
        Expr::String(s) => {
            let valid = torajs_str::language_tag::is_structurally_valid_language_tag(s.as_bytes());
            Some((s.clone(), valid))
        }
        _ => None,
    }
}

fn call_by_name(ctx: &mut LowerCtx<'_>, name: &str, argv: Vec<Operand>) {
    let fid = *ctx.fn_table.get(name).expect("locale intrinsic missing");
    ctx.f
        .append_inst(ctx.cur_block, InstKind::Call(fid, argv), Type::Void, None);
}

/// Lower the arguments of a Number / BigInt (`date` None) or Date
/// (`date` = receiver and kind) `toLocale*String` call and check them
/// as the ECMA-402 constructor would. No arguments, no check.
pub(crate) fn emit_locale_args_check(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    date: Option<(Operand, i64)>,
) {
    let literal = args.first().and_then(|&a| literal_tag(ctx, a));
    if args.len() == 1 && literal.as_ref().is_some_and(|(_, valid)| *valid) {
        return;
    }
    let Some(mut la) = lower_locale_args(ctx, args) else {
        return;
    };
    match (literal, date) {
        (Some((tag, false)), None) => {
            let tag = ctx.intern_string_literal(&tag);
            call_by_name(
                ctx,
                "__torajs_str_locale_invalid",
                vec![Operand::Value(tag)],
            );
        }
        (literal, date) => {
            if literal.is_some_and(|(_, valid)| valid) {
                la.locales = box_undefined(ctx);
            }
            match date {
                Some((d, kind)) => call_by_name(
                    ctx,
                    "__torajs_locale_args_check_date",
                    vec![d, la.locales, la.options, Operand::ConstI64(kind)],
                ),
                None => call_by_name(
                    ctx,
                    "__torajs_locale_args_check",
                    vec![la.locales, la.options, Operand::ConstI64(0)],
                ),
            }
        }
    }
    release_locale_args(ctx, &la);
    ctx.emit_throw_check(None);
}

/// A Number / BigInt `toLocaleString(...)`: the argument check, then
/// the en-US formatter `fmt` on the receiver.
pub(crate) fn gated_format(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    fmt: FuncId,
    recv_op: Operand,
) -> Operand {
    emit_locale_args_check(ctx, args, None);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fmt, vec![recv_op]),
        Type::Str,
        None,
    );
    Operand::Value(v)
}
