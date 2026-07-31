//! `RegExp.escape(x)` direct-call lowering (rotation 266 — ES2025
//! §22.2.5.1). One arm: box the argument to Any and ride the
//! strict-String any shell (`__torajs_regexp_escape_any`) — step 1's
//! "not a String → TypeError" gate and the EncodeForRegExpEscape
//! kernel both live runtime-side, so the boxed fresh Str comes back
//! as the Any answer and a non-string records a catchable throw.
//!
//! Returns `Some(op)` on hit; `None` when the callee isn't the
//! `RegExp.escape` Member-Ident shape or the args list is empty.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "escape" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "RegExp" || args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    let any_op = if matches!(arg_ty, Type::Any) {
        arg_op
    } else {
        ctx.box_to_any_from_expr(args[0], arg_op)
    };
    for a in args.iter().skip(1) {
        let _ = ctx.lower_expr(*a);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.regexp_escape_any, vec![any_op]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
