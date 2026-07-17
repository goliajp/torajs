//! Annex B B.2.2 `String.prototype` HTML method lowering —
//! `s.anchor(v)` / `s.bold()` and the other eleven CreateHTML
//! wrappers, dispatched to the `torajs-str/html.rs` kernels.
//!
//! All thirteen share one shape: the receiver (Str, or Substr
//! materialized to owned) plus, for the four attributed forms
//! (anchor / fontcolor / fontsize / link), one attribute value
//! coerced to Str — B.2.2.2.1 runs ToString on it unconditionally,
//! so a Number / Bool / Any argument is legal (`"x".fontsize(7)`).
//! Spec reserves no further slots; every extra argument is lowered
//! for side effects and dropped (S272 idiom). A missing attribute
//! value lowers to ConstPtrNull — the kernel renders the JS
//! `undefined` text per the RC-4 F1b-2 NULL convention.

use crate::ast::ExprId;
use crate::check;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `Some(tag_fid_picker)` when `method` is an HTML method; the bool
/// is `true` for the four attributed forms.
fn html_method_takes_attr(method: &str) -> Option<bool> {
    match method {
        "anchor" | "fontcolor" | "fontsize" | "link" => Some(true),
        "big" | "blink" | "bold" | "fixed" | "italics" | "small" | "strike" | "sub" | "sup" => {
            Some(false)
        }
        _ => None,
    }
}

pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    let takes_attr = html_method_takes_attr(method)?;
    if !matches!(recv_ty, Type::Str | Type::Substr) {
        return None;
    }
    // Substr receiver materializes to an owned Str for the kernel's
    // (header, len, payload) reads; released after the call.
    let (recv, recv_owned) = if recv_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![recv_op]),
            Type::Str,
            None,
        );
        (Operand::Value(owned), true)
    } else {
        (recv_op, false)
    };
    let mut argv = vec![recv];
    let mut temp: Option<Operand> = None;
    if takes_attr {
        if let Some(&a) = args.first() {
            let expr_ty = ctx.expr_types.get(&a).cloned();
            if matches!(expr_ty, Some(check::Type::Undefined)) {
                // ToString(undefined) — let the kernel's NULL arm
                // render the literal text.
                argv.push(Operand::ConstPtrNull);
            } else if matches!(expr_ty, Some(check::Type::Any)) {
                let v = ctx.lower_expr(a);
                let s = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_str_box, vec![v]),
                    Type::Str,
                    None,
                );
                // Pending-throw propagation — see stmt_return's twin
                // (0-check audit, rotation 130 L3b).
                ctx.emit_throw_check(None);
                temp = Some(Operand::Value(s));
                argv.push(Operand::Value(s));
            } else {
                let v = ctx.lower_expr(a);
                let (s, fresh) =
                    crate::ssa_lower_binop_inner::add_str::coerce_to_str(ctx, v, false);
                if fresh {
                    temp = Some(s);
                }
                argv.push(s);
            }
        } else {
            argv.push(Operand::ConstPtrNull);
        }
    }
    // Trailing args (arg 1.. for attributed forms, all args for the
    // plain wraps) — lower for side effects, drop the values.
    let trailing_from = usize::from(takes_attr);
    for &a in args.iter().skip(trailing_from) {
        let _ = ctx.lower_expr(a);
    }
    let target = match method {
        "anchor" => ctx.intrinsics.str_anchor,
        "fontcolor" => ctx.intrinsics.str_fontcolor,
        "fontsize" => ctx.intrinsics.str_fontsize,
        "link" => ctx.intrinsics.str_link,
        "big" => ctx.intrinsics.str_big,
        "blink" => ctx.intrinsics.str_blink,
        "bold" => ctx.intrinsics.str_bold,
        "fixed" => ctx.intrinsics.str_fixed,
        "italics" => ctx.intrinsics.str_italics,
        "small" => ctx.intrinsics.str_small,
        "strike" => ctx.intrinsics.str_strike,
        "sub" => ctx.intrinsics.str_sub,
        "sup" => ctx.intrinsics.str_sup,
        _ => unreachable!(),
    };
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(target, argv), Type::Str, None);
    // The kernel is read-only on its Str args — settle the coerce
    // temp and the materialized Substr receiver.
    if let Some(t) = temp {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_drop, vec![t]),
        );
    }
    if recv_owned {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_drop, vec![recv]),
        );
    }
    Some(Operand::Value(v))
}
