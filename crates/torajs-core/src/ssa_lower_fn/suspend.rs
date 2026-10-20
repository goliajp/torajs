//! `await` in the body of an async function that suspends over a
//! frame (`ast::desugar_async_ssa_gate` chose it): each await becomes
//! an `InstKind::Suspend` the split pass (torajs-egraph
//! `suspend_split`) turns into a park and a landing.
//!
//! ECMA-262 §27.7.5.3 Await, by the operand's static type:
//!
//! - a built-in promise: the cell itself is awaited (PromiseResolve of
//!   a %Promise% answers it; its `constructor` cannot be replaced
//!   under a static `Promise<T>`). The suspension answers the settled
//!   cell, and its value is read in the inner lane like the draining
//!   read does (`ssa_lower_member_promise_value`), a rejection raising
//!   through the read;
//! - `any`, or any other object (a function included): PromiseResolve
//!   through the any-lane kernel (a native cell answers itself, an
//!   object whose `then` is callable gets its job, anything else a
//!   fulfilled cell), then as above in the `any` lane, the await's
//!   type (`check::awaited`);
//! - a primitive (§27.2.1.3.2 step 8: a resolution that is not an
//!   Object fulfills): a direct suspension one job long, and the
//!   await's value is the operand.
//!
//! Every suspension is followed by a throw check: a frame dropped
//! while it waits is unwound by re-entering it there with a throw
//! pending (`torajs-promise` `frame::body`), so the body's exception
//! path releases what it holds. What that path releases is also what
//! the suspension lists as held, for the frame's trace to report.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, SuspendKind, SuspendLane, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_member_promise_value as pv;
use crate::ssa_lower_temp_scratch::ThrowTemp;

/// Does `obj` read a promise some binding or container still holds,
/// so the await takes a reference of its own? An assertion (`h as
/// Promise<number>`) reads what its operand reads.
fn borrow_shaped(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::As { expr, .. } => borrow_shaped(ctx, *expr),
        e => matches!(e, Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }),
    }
}

pub(crate) fn lower_await(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId) -> Operand {
    if pv::detect_obj_is_builtin_promise(ctx, obj) {
        let src = ctx.lower_expr(obj);
        if borrow_shaped(ctx, obj) {
            ctx.emit_owned_result_inc(src, Type::Promise);
        }
        let inner = inner_lane(ctx, eid, obj);
        return await_cell(ctx, src, inner);
    }
    let op = ctx.lower_expr(obj);
    match ctx.operand_ty(&op) {
        Type::Promise => {
            if borrow_shaped(ctx, obj) {
                ctx.emit_owned_result_inc(op, Type::Promise);
            }
            let inner = inner_lane(ctx, eid, obj);
            await_cell(ctx, op, inner)
        }
        Type::Any => {
            if !ctx.expr_transfers_ownership(obj) {
                ctx.emit_owned_result_inc(op, Type::Any);
            }
            await_any(ctx, op)
        }
        ty if is_primitive(ty) => await_direct(ctx, eid, obj, op, ty),
        // an object: its `then` is looked up when it settles
        _ => {
            let borrowed = !ctx.expr_transfers_ownership(obj);
            let boxed = ctx.box_owned_from_expr(obj, op, borrowed);
            await_any(ctx, boxed)
        }
    }
}

/// The lanes no thenable comes in.
fn is_primitive(ty: Type) -> bool {
    matches!(
        ty,
        Type::I64
            | Type::F64
            | Type::I32
            | Type::Bool
            | Type::Str
            | Type::Substr
            | Type::BigInt
            | Type::Symbol
            | Type::Void
            | Type::Ptr
    )
}

/// The lane an awaited promise's value is read in: the await's own
/// type when that is `any` (a promise of a thenable), else the
/// promise's inner type at its value point's width.
fn inner_lane(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId) -> Option<Type> {
    if awaits_any(ctx, eid) {
        return Some(Type::Any);
    }
    let inner = pv::recover_inner_ssa_ty(ctx, obj);
    ctx.widen_promise_inner_ty(inner, obj)
}

fn awaits_any(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any))
}

/// PromiseResolve of the owned `any` box `op` through the any-lane
/// kernel, then the cell's await in the `any` lane.
fn await_any(ctx: &mut LowerCtx<'_>, op: Operand) -> Operand {
    let cb = ctx.cur_block;
    let cell = ctx.f.append_inst(
        cb,
        InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![op]),
        Type::Promise,
        None,
    );
    // the thenable's `then` read may raise
    ctx.emit_throw_check(None);
    await_cell(ctx, Operand::Value(cell), Some(Type::Any))
}

/// Await the owned cell `src`, then read its settled value in the
/// `inner` lane (the read releases nothing; `src` is dropped after).
fn await_cell(ctx: &mut LowerCtx<'_>, src: Operand, inner: Option<Type>) -> Operand {
    let settled = suspend_on(ctx, src, inner.unwrap_or(Type::Any));
    let raw = read_settled(ctx, settled, pv::awaited_lane_repr(inner.as_ref()));
    let v = pv::cast_promise_value(ctx, raw, inner);
    ctx.emit_drop_value(settled, Type::Promise);
    Operand::Value(v)
}

/// The suspension on the owned cell `src`; answers the settled cell,
/// owned.
fn suspend_on(ctx: &mut LowerCtx<'_>, src: Operand, lane: Type) -> Operand {
    let held = held(ctx, None);
    let cb = ctx.cur_block;
    let kind = InstKind::Suspend(SuspendKind::Await, SuspendLane::Promise(lane), src, held);
    let settled = Operand::Value(ctx.f.append_inst(cb, kind, Type::Promise, None));
    ctx.emit_throw_check_owned(None, settled, Type::Promise);
    settled
}

/// The settled cell's value word as `want_repr` asks for it; a
/// rejected source raises its reason here.
fn read_settled(ctx: &mut LowerCtx<'_>, settled: Operand, want_repr: i64) -> crate::ssa::ValueId {
    let cb = ctx.cur_block;
    let raw = ctx.f.append_inst(
        cb,
        InstKind::Call(
            ctx.intrinsics.promise_get_value_as,
            vec![settled, Operand::ConstI64(want_repr)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check_owned(None, settled, Type::Promise);
    raw
}

/// Await a value that is not a thenable: the body keeps the operand
/// across the suspension, as its own reference (a borrowed read takes
/// one, since the binding it came from may change while the function
/// waits), and the await answers it owned.
fn await_direct(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    op: Operand,
    ty: Type,
) -> Operand {
    let owned = !ty.is_copy();
    if owned && !ctx.expr_transfers_ownership(obj) {
        ctx.emit_owned_result_inc(op, ty);
    }
    let held = held(ctx, owned.then_some((op, ty)));
    let cb = ctx.cur_block;
    let kind = InstKind::Suspend(SuspendKind::Await, SuspendLane::Direct, op, held);
    ctx.f.append_void(cb, kind);
    if owned {
        ctx.emit_throw_check_owned(None, op, ty);
        ctx.owned_member_reads.insert(eid);
    } else {
        ctx.emit_throw_check(None);
    }
    op
}

/// What the body owns at a suspension point: what the throw check
/// after it releases (no `try` is open around an await here), that is
/// the owned locals, the temps parked for a throw and the operand the
/// suspension keeps. The frame's trace reports them while the function
/// waits there. Left out: a substring view (no cycle passes through
/// one, and it has no universal header to read), and whatever the
/// throw path releases by another protocol than a drop of the value
/// it holds (capture boxes, class cells, relocation slots).
fn held(ctx: &mut LowerCtx<'_>, kept: Option<(Operand, Type)>) -> Vec<Operand> {
    let traced = |ty: Type| !ty.is_copy() && ty != Type::Substr;
    let mut slots: Vec<(crate::ssa::ValueId, Type)> = (ctx.locals.iter())
        .filter(|(name, info)| {
            traced(info.ty)
                && !info.moved
                && !ctx.stack_alloced_locals.contains(*name)
                && !ctx.boxed_noncopy_lets.contains(*name)
                && !(info.ty == Type::Any
                    && crate::ssa_lower_closure::class_sentinel_name(ctx, name))
        })
        .map(|(_, info)| (info.slot, info.ty))
        .collect();
    let temps = ctx.temps.throw_live.iter().flatten();
    let mut held: Vec<Operand> = Vec::new();
    for t in temps {
        match t {
            ThrowTemp::Value(op, ty) if traced(*ty) => held.push(*op),
            ThrowTemp::Slot(slot, ty) if traced(*ty) => slots.push((*slot, *ty)),
            _ => {}
        }
    }
    slots.sort_by_key(|(s, _)| s.0);
    slots.dedup_by_key(|(s, _)| s.0);
    let cb = ctx.cur_block;
    for (slot, ty) in slots {
        let load = InstKind::Load(ty, Operand::Value(slot), 0);
        held.push(Operand::Value(ctx.f.append_inst(cb, load, ty, None)));
    }
    if let Some((op, ty)) = kept
        && traced(ty)
    {
        held.push(op);
    }
    held
}
