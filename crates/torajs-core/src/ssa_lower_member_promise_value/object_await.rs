//! `await` of a value that is not a promise, outside a frame.

use super::{awaited_lane_repr, cast_promise_value};
use crate::ast::ExprId;
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `await <object>` outside a frame (§27.7.5.3 step 2): PromiseResolve
/// of the owned box `boxed` through the any-lane kernel (a callable
/// `then` gets its job, anything else a fulfilled cell), then the
/// draining read of that cell in the `any` lane.
pub(super) fn lower_object_await(ctx: &mut LowerCtx<'_>, boxed: Operand) -> Operand {
    let cb = ctx.cur_block;
    let cell = ctx.f.append_inst(
        cb,
        InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![boxed]),
        Type::Promise,
        None,
    );
    let cell = Operand::Value(cell);
    // a `then` that throws leaves a rejected cell, read below; the
    // check stays as on the frame path
    ctx.emit_throw_check(None);
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Call(ctx.intrinsics.microtask_drain, vec![]));
    let want_repr = awaited_lane_repr(Some(&Type::Any));
    let raw = ctx.f.append_inst(
        cb,
        InstKind::Call(
            ctx.intrinsics.promise_get_value_as,
            vec![cell, Operand::ConstI64(want_repr)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check_owned(None, cell, Type::Promise);
    let v = cast_promise_value(ctx, raw, Some(Type::Any));
    ctx.emit_drop_value(cell, Type::Promise);
    Operand::Value(v)
}

/// `await e` outside a frame typed as `e`'s own type: a primitive, or
/// an object by a closed `then` proof (`check::awaited`). A primitive
/// answers itself; an object settles through PromiseResolve like the
/// `any` lane above (a promise cell under a type that lies is adopted)
/// and its value is taken back as the object type. A function pointer
/// is no cell and no promise: the jobs run, and it answers itself.
pub(super) fn lower_typed_await(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    op: Operand,
) -> Operand {
    let ty = ctx.operand_ty(&op);
    let lane = match ty {
        Type::FnSig(_) => {
            let cb = ctx.cur_block;
            ctx.f
                .append_void(cb, InstKind::Call(ctx.intrinsics.microtask_drain, vec![]));
            return op;
        }
        Type::Inline(sid) => Type::Obj(sid),
        t if crate::ssa_lower_fn::suspend::is_object_cell(t) => t,
        _ => return op,
    };
    let borrowed = !ctx.expr_transfers_ownership(obj);
    let boxed = ctx.box_owned_from_expr(obj, op, borrowed);
    ctx.owned_member_reads.insert(eid);
    let v = lower_object_await(ctx, boxed);
    crate::ssa_lower_fn::suspend::cell_of(ctx, v, lane)
}

pub(super) fn try_p10_4_primitive_identity(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Operand> {
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(check_mod::Type::Number)
            | Some(check_mod::Type::String)
            | Some(check_mod::Type::Boolean)
            | Some(check_mod::Type::Array(_))
            | Some(check_mod::Type::BigInt)
    ) {
        return None;
    }
    Some(ctx.lower_expr(obj))
}
