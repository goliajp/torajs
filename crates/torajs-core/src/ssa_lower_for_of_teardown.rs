//! §7.4.9 IteratorClose + the iterator slot's release, as a record the
//! loop's exit block and a `return` out of the loop body can both
//! emit.
//!
//! `break` reaches the close because it branches to the loop's exit
//! block, which is where the close lives (RFC 20260725 刀 6 / 6b).
//! `return` doesn't — it leaves for the function epilogue and the
//! iterator is neither closed nor released. Parking the two lanes'
//! teardown on a stack lets the return site emit the same sequence at
//! whatever depth of nested loops it is leaving.
//!
//! The exit block guards the close on "did the loop stop early"; a
//! return needs no guard, since returning IS stopping early.

use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// One live for-of's teardown, by lane. Both carry the iterator slot;
/// they differ in how the iterator is reached — the any lane hands the
/// source alongside it (the kernel picks the protocol back up from
/// there), the typed lane knows the iterator's static type and boxes
/// the loaded value.
#[derive(Clone)]
pub(crate) enum ForOfTeardown {
    Any {
        src: Operand,
        iter_slot: ValueId,
    },
    Typed {
        iter_slot: ValueId,
        iter_ret_ty: Type,
    },
}

/// §7.4.9 with the iterator still live. An iterator with no `return`
/// closes as a no-op, and so does a loop that never derived one (an
/// indexed receiver leaves the slot at `undefined`), so this is safe
/// for every lane the kernel drives.
pub(crate) fn emit_close(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    match td {
        ForOfTeardown::Any { src, iter_slot } => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_iter_close,
                    vec![src.clone(), Operand::Value(*iter_slot)],
                ),
            );
        }
        ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        } => {
            let iter_for_close = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*iter_ret_ty, Operand::Value(*iter_slot), 0),
                *iter_ret_ty,
                None,
            );
            let boxed_iter = ctx.box_to_any(Operand::Value(iter_for_close));
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.iter_close_value, vec![boxed_iter]),
            );
        }
    }
    ctx.emit_throw_check(None);
}

/// The slot's own stake. Independent of the close — a loop that ran to
/// completion still holds the iterator reference.
pub(crate) fn emit_release(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    let (slot, ty) = match td {
        ForOfTeardown::Any { iter_slot, .. } => (*iter_slot, Type::Any),
        ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        } => (*iter_slot, *iter_ret_ty),
    };
    let val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(ty, Operand::Value(slot), 0),
        ty,
        None,
    );
    ctx.emit_drop_value(Operand::Value(val), ty);
}

/// Every enclosing for-of, innermost first — the LIFO a `return` out
/// of nested loops has to unwind. Emitted into the current block, so
/// the caller owns the placement.
pub(crate) fn emit_all_for_return(ctx: &mut LowerCtx) {
    let pending: Vec<ForOfTeardown> = ctx.for_of_teardown_stack.iter().rev().cloned().collect();
    for td in &pending {
        emit_close(ctx, td);
        emit_release(ctx, td);
    }
}
