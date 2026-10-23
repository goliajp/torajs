//! What the body of an async function holds at a suspension point, for
//! the frame's trace (`super`).

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

/// What the body owns at a suspension point: what a cancelled frame's
/// exit after it releases (`cancel`), that is the owned locals, the
/// temps parked for a throw and the operand the suspension keeps. The
/// frame's trace reports them while the function waits there. Left
/// out: a substring view (no cycle passes through one, and it has no
/// universal header to read), whatever the throw path releases by
/// another protocol than a drop of the value it holds (capture boxes,
/// class cells, relocation slots), and what a `finally` body around
/// the point may keep aside (a pending return, a suspended throw):
/// whether those slots hold anything is known only at run time, so a
/// frame the cycle collector frees there does not release them.
pub(super) fn held(ctx: &mut LowerCtx<'_>, kept: Option<(Operand, Type)>) -> Vec<Operand> {
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
