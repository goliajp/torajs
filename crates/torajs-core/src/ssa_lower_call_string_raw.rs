//! `String.raw(template, ...substitutions)` lowering — dispatch the
//! direct-fn call shape to the `__torajs_string_raw` runtime kernel.
//!
//! The kernel walks `template.raw` and interleaves substitutions,
//! coercing every part through ToString. We hand it a boxed template
//! (shape-blind — any object with `raw` array is accepted at runtime)
//! and a stack-allocated `argv` of AnyValue subs (via the shared
//! [`crate::ssa_lower_any_method_call::pack_any_argv`] helper the
//! variadic-boxed-call path already uses).
//!
//! Zero subs are legal (`String.raw({raw:['x']})` answers `"x"`); the
//! kernel walks `raw.length` and skips the interleave when
//! `argc == 0`. The tagged-template literal surface
//! `String.raw\`x${y}\`` is separate parser + emitter substrate not
//! covered here.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "raw" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "String" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // Template — coerce to Any so the runtime member-get walk fits
    // every shape (dynobj / struct / class instance). A borrow-shape
    // template needs +1 for the box (the caller may still hold the
    // binding); an owned literal transfers.
    let tmpl_eid = args[0];
    let tmpl_raw = ctx.lower_expr(tmpl_eid);
    let tmpl_ty = ctx.operand_ty(&tmpl_raw);
    let (tmpl_op, we_boxed_tmpl) = if matches!(tmpl_ty, Type::Any) {
        (tmpl_raw, false)
    } else {
        if !ctx.expr_transfers_ownership(tmpl_eid) && tmpl_ty.is_refcounted() {
            ctx.emit_rc_inc(tmpl_raw.clone());
        }
        (ctx.box_to_any(tmpl_raw), true)
    };
    // Subs — pack via the shared any-method-call helper so a borrow
    // slot takes its own +1 and an owned temp transfers, exactly the
    // ledger the variadic-boxed-call path already carries.
    // Rotation 550 — the template we hold (our box, or an owned temp
    // released below) is live across the subs' lowers; park it.
    let tmpl_tok = if we_boxed_tmpl {
        Some(ctx.push_throw_temp(tmpl_op.clone(), Type::Any))
    } else {
        ctx.park_owned_temp(tmpl_eid, &tmpl_op)
    };
    let packed = crate::ssa_lower_any_method_call::pack_any_argv(ctx, &args[1..]);
    let subs_argv = packed.argv;
    let subs_argc = (args.len() - 1) as i64;
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.string_raw,
            vec![
                tmpl_op.clone(),
                Operand::Value(subs_argv),
                Operand::ConstI64(subs_argc),
            ],
        ),
        Type::Str,
        None,
    );
    // Post-call release — kernel only borrows subs boxes; each slot
    // WE boxed drops here. Sibling variadic-call path drops the same
    // way.
    packed.release(ctx);
    ctx.unpark_owned_temp(tmpl_tok);
    // Template we boxed vs template already Any — the boxed one is
    // ours to release; a borrow-shape original stays owned by its
    // binding and released elsewhere.
    if we_boxed_tmpl {
        ctx.emit_drop_value(tmpl_op, Type::Any);
    } else {
        ctx.release_owned_temp(tmpl_eid, &tmpl_op);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
