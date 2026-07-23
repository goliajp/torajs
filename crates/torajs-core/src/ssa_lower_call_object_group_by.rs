//! `Object.groupBy(items, callbackFn)` lowering per ES §20.1.2.10
//! — Array items lane (iterable-only receivers deferred L3b).
//!
//! Dispatches to `__torajs_object_group_by` (torajs-meta). The
//! kernel walks the array and invokes cb via the uniform any-call
//! ABI, so both args box to Any:
//! - items: Arr / Any pass through the `emit_arr_mark_kind` +
//!   PtrToInt/IntToPtr shape (mirror of the fromEntries arm — the
//!   walker's kind-aware borrowed reads need a self-describing
//!   block).
//! - cb: any closure-shaped receiver; `box_to_any` on the caller
//!   side lands the closure cell in an ANY_HEAP slot the kernel's
//!   `__torajs_any_call` dispatches through the boxed dual entry.
//!
//! Return is a fresh null-prototype dynobj (kernel owned +1) —
//! surfaces as `Type::Any` from the checker wedge; downstream
//! member reads / prints ride the existing dynobj lanes.

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
    if name != "groupBy" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    // items — box to Any. Static Arr receivers stamp their elem
    // kind before the PtrToInt bridge (mirror fromEntries) so the
    // kernel's kind-aware slot reads see a self-describing block.
    let items_eid = args[0];
    let items_raw = ctx.lower_expr(items_eid);
    let items_ty = ctx.operand_ty(&items_raw);
    let items_op: Operand = match items_ty {
        Type::Any => items_raw.clone(),
        Type::Arr(_) => {
            ctx.emit_arr_mark_kind(&items_raw);
            let as_i64 = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::PtrToInt(items_raw.clone()),
                Type::I64,
                None,
            );
            let as_any = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::IntToPtr(Operand::Value(as_i64)),
                Type::Any,
                None,
            );
            Operand::Value(as_any)
        }
        other => panic!("ssa-lower: unsupported items shape for Object.groupBy: {other:?}"),
    };
    // cb — box to Any. A borrow-shape callee needs +1 (the caller
    // still holds the binding); an owned temp transfers. Same
    // ledger the sibling String.raw arm uses on its template arg.
    let cb_eid = args[1];
    let cb_raw = ctx.lower_expr(cb_eid);
    let cb_ty = ctx.operand_ty(&cb_raw);
    let (cb_op, we_boxed_cb) = if matches!(cb_ty, Type::Any) {
        (cb_raw, false)
    } else {
        if !ctx.expr_transfers_ownership(cb_eid) && cb_ty.is_refcounted() {
            ctx.emit_rc_inc(cb_raw.clone());
        }
        (ctx.box_to_any(cb_raw), true)
    };
    // Silently ignore trailing args per spec (their side effects
    // still fire).
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.object_group_by,
            vec![items_op.clone(), cb_op.clone()],
        ),
        Type::Any,
        None,
    );
    // Kernel borrows items (per-slot reads never consume); an
    // owned-shape items temp still holds its own stake — release
    // it (fromEntries container-store family precedent).
    ctx.release_owned_temp(items_eid, &items_op);
    // Kernel borrows cb (dispatches once per item, never consumes).
    if we_boxed_cb {
        ctx.emit_drop_value(cb_op, Type::Any);
    } else {
        ctx.release_owned_temp(cb_eid, &cb_op);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(result))
}
