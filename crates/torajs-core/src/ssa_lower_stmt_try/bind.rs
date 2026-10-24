//! The catch binding's take of the pending throw: into an `any`
//! parameter, into a typed one, or into nothing for a `catch {}`.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Box the pending throw for an `any` catch parameter `p`.
pub(super) fn take_any(ctx: &mut LowerCtx, p: &str) -> Operand {
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        Some(p),
    );
    let boxed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(boxed)
}

/// Take the pending throw's raw 8 bytes into a parameter `p` of `e_ty`.
pub(super) fn take_raw(ctx: &mut LowerCtx, p: &str, e_ty: Type) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        Some(p),
    );
    if e_ty == Type::F64 {
        // The slot holds raw 8 bytes; decode them the way the
        // throw site encoded them rather than converting the
        // number (the symmetric `BitCastF64ToI64` the promise
        // value point uses).
        Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BitCastI64ToF64(Operand::Value(v)),
            Type::F64,
            None,
        ))
    } else {
        Operand::Value(v)
    }
}

/// Release the pending throw of a catch that binds nothing.
pub(super) fn release_unbound(ctx: &mut LowerCtx) {
    // Unbound `catch {}` still owns the taken value — take
    // (tag, value), box, and drop it. Pre-fix this arm only
    // cleared the active flag via a discarded throw_take,
    // stranding the whole thrown heap payload (Error inst +
    // message + stack) per catch.
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        None,
    );
    let boxed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
}
