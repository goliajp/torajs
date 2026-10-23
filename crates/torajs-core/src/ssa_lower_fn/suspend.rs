//! `await` in the body of an async function that suspends over a
//! frame (`ast::desugar_async_ssa_gate` chose it): each await becomes
//! an `InstKind::Suspend` the split pass (torajs-egraph
//! `suspend_split`) turns into a park and a landing. The module body
//! awaits in place instead (`in_place`).
//!
//! ECMA-262 §27.7.5.3 Await, by the operand's static type:
//!
//! - a built-in promise: the cell itself is awaited (PromiseResolve of
//!   a %Promise% answers it; its `constructor` cannot be replaced
//!   under a static `Promise<T>`). The suspension answers the settled
//!   cell, and its value is read in the inner lane like the draining
//!   read does (`ssa_lower_member_promise_value`), a rejection raising
//!   through the read;
//! - an object the await is typed as (`check::awaited`: its type has
//!   no `then` and the program is proven to give no object one): a
//!   promise cell under a type that lies (an inferred slot that took
//!   an `any`) is awaited as the cell and read in the object's lane,
//!   any other value
//!   is kept across a direct suspension one job long, the cell's tag
//!   telling the two apart (`object`);
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
//! path releases what it holds (inside a `try` or a `for…of`, an exit
//! of its own that runs no handler, `cancel`). What that path releases
//! is also what the suspension lists as held, for the frame's trace to
//! report.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, SuspendKind, SuspendLane, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_member_promise_value as pv;

mod cancel;
mod held;
mod in_place;
mod object;
mod read;
mod then_fused;

pub(crate) use cancel::CANCELLED_PROBE;
use held::held;
pub(crate) use object::{cell_of, is_object_cell};
use read::read_settled;

/// The suspension releases the cell it awaited, so an operand that does
/// not hand over a reference of its own (a promise a binding, a field
/// or an element still holds, under whatever comma, assignment,
/// conditional, `??` or `?.` reads it) is taken one first.
pub(crate) fn lower_await(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId) -> Operand {
    if in_place::in_module_body(ctx)
        && let Some(v) = then_fused::lower(ctx, eid, obj)
    {
        return v;
    }
    if pv::detect_obj_is_builtin_promise(ctx, obj) {
        let src = ctx.lower_expr(obj);
        // an element of an array the `any` side reached is held as
        // `any`, whatever that side stored: PromiseResolve takes it
        let held_any = matches!(ctx.operand_ty(&src), Type::Any);
        if !ctx.expr_transfers_ownership(obj) {
            let ty = if held_any { Type::Any } else { Type::Promise };
            ctx.emit_owned_result_inc(src, ty);
        }
        let inner = inner_lane(ctx, eid, obj);
        if held_any {
            return await_any_in(ctx, src, inner.unwrap_or(Type::Any));
        }
        let undef = pv::undef_cell_slot(ctx, obj, inner.as_ref());
        return await_cell(ctx, src, Some(obj), inner, undef);
    }
    let op = ctx.lower_expr(obj);
    match ctx.operand_ty(&op) {
        Type::Promise => {
            if !ctx.expr_transfers_ownership(obj) {
                ctx.emit_owned_result_inc(op, Type::Promise);
            }
            let inner = inner_lane(ctx, eid, obj);
            let undef = pv::undef_cell_slot(ctx, obj, inner.as_ref());
            await_cell(ctx, op, Some(obj), inner, undef)
        }
        Type::Any => {
            if !ctx.expr_transfers_ownership(obj) {
                ctx.emit_owned_result_inc(op, Type::Any);
            }
            await_any(ctx, op)
        }
        ty if is_primitive(ty) => await_direct(ctx, eid, obj, op, ty),
        ty if !awaits_any(ctx, eid) => object::await_object(ctx, eid, obj, op, ty),
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
    await_any_in(ctx, op, Type::Any)
}

/// PromiseResolve of the owned `any` box `op`, then the cell's await
/// read in the `lane` its await is typed as.
fn await_any_in(ctx: &mut LowerCtx<'_>, op: Operand, lane: Type) -> Operand {
    let cb = ctx.cur_block;
    let call = InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![op]);
    let cell = ctx.f.append_inst(cb, call, Type::Promise, None);
    // the thenable's `then` read may raise
    ctx.emit_throw_check(None);
    await_cell(ctx, Operand::Value(cell), None, Some(lane), None)
}

/// Await the owned cell `src`, the value of the operand `obj` when
/// there is one, then read its settled value in the `inner` lane (the
/// read releases nothing; `src` is dropped after); `undef` names the
/// slot whose undefined cell an undefined result is answered as
/// (`pv::undef_cell_slot`).
fn await_cell(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    obj: Option<ExprId>,
    inner: Option<Type>,
    undef: Option<Type>,
) -> Operand {
    let want = pv::awaited_lane_repr(inner.as_ref());
    let (settled, raw) = if in_place::in_module_body(ctx) {
        // the module body awaits in place; the cell it holds settles
        (src, in_place::await_read(ctx, src, obj, want, false))
    } else {
        let settled = suspend_on(ctx, src, inner.unwrap_or(Type::Any));
        (settled, read_settled(ctx, settled, want))
    };
    let raw = match undef {
        Some(slot) => pv::keep_undefined(ctx, settled, raw, slot),
        None => raw,
    };
    let v = pv::cast_promise_value(ctx, raw, inner);
    ctx.emit_drop_value(settled, Type::Promise);
    Operand::Value(v)
}

/// The suspension on the owned cell `src`; answers the settled cell,
/// owned.
fn suspend_on(ctx: &mut LowerCtx<'_>, src: Operand, lane: Type) -> Operand {
    let aside = held::aside(ctx);
    let held = held(ctx, None, &aside);
    let cb = ctx.cur_block;
    let kind = InstKind::Suspend(SuspendKind::Await, SuspendLane::Promise(lane), src, held);
    let settled = Operand::Value(ctx.f.append_inst(cb, kind, Type::Promise, None));
    cancel::throw_check(ctx, Some((settled, Type::Promise)), &aside);
    settled
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
    if !ty.is_copy() && !ctx.expr_transfers_ownership(obj) {
        ctx.emit_owned_result_inc(op, ty);
    }
    suspend_direct(ctx, eid, op, ty)
}

/// The direct suspension over the operand `op`, owned when its type
/// holds a reference; the await answers it. The module body awaits in
/// place (`in_place`).
fn suspend_direct(ctx: &mut LowerCtx<'_>, eid: ExprId, op: Operand, ty: Type) -> Operand {
    let owned = !ty.is_copy();
    if in_place::in_module_body(ctx) {
        in_place::yield_direct(ctx);
        if owned {
            ctx.owned_member_reads.insert(eid);
        }
        return op;
    }
    let aside = held::aside(ctx);
    let held = held(ctx, owned.then_some((op, ty)), &aside);
    let cb = ctx.cur_block;
    let kind = InstKind::Suspend(SuspendKind::Await, SuspendLane::Direct, op, held);
    ctx.f.append_void(cb, kind);
    cancel::throw_check(ctx, owned.then_some((op, ty)), &aside);
    if owned {
        ctx.owned_member_reads.insert(eid);
    }
    op
}
