//! `Map.groupBy(items, callbackFn)` lowering per ES §24.2.2.4 —
//! Array items lane (iterable-only receivers deferred L3b).
//!
//! Sister to [`crate::ssa_lower_call_object_group_by`]; same items
//! + cb boxing shape, dispatches to `__torajs_map_group_by`
//! (torajs-meta) which walks the array + accumulates into a Map
//! with SameValueZero-keyed `Array<Any>` buckets.
//!
//! Return is a fresh Map (kernel owned +1) — surfaces as
//! `Type::Map` from the checker wedge.

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
    if ns != "Map" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    // items — box to Any (mirror sibling Object.groupBy arm).
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
        other => panic!("ssa-lower: unsupported items shape for Map.groupBy: {other:?}"),
    };
    // cb — box to Any (mirror sibling arm).
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
    // Trailing args — spec ignores; still lower for side effects.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    // Kernel returns a fresh Map cell as raw AnyValue bits — surface
    // as Type::Map (a Map heap pointer already satisfies the cell
    // encoding — IntToPtr in reverse of the fromEntries bridge).
    let any_bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_group_by,
            vec![items_op.clone(), cb_op.clone()],
        ),
        Type::Any,
        None,
    );
    let as_i64 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::PtrToInt(Operand::Value(any_bits)),
        Type::I64,
        None,
    );
    let as_map = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::IntToPtr(Operand::Value(as_i64)),
        Type::Map,
        None,
    );
    // Kernel borrows items + cb per iteration (never consumes).
    ctx.release_owned_temp(items_eid, &items_op);
    if we_boxed_cb {
        ctx.emit_drop_value(cb_op, Type::Any);
    } else {
        ctx.release_owned_temp(cb_eid, &cb_op);
    }
    ctx.emit_throw_check(None);
    Some(Operand::Value(as_map))
}
