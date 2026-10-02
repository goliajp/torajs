//! The typed grow-kernel call shared by the push lanes — moved out of
//! `ssa_lower_call_arr_push.rs` at the 500-line limit. The host decides
//! which receiver shapes a push takes; this file emits the append and
//! settles the pushed value's stake on both edges.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// The typed grow-kernel call (`arr_push` / `arr_unshift`) with its
/// throw check and the value's stake settled on both edges.
///
/// The kernels refuse a non-extensible receiver by recording a
/// TypeError and storing nothing (`__torajs_arr_extend_guard`). They
/// are intrinsics, which `emit_throw_check(Some)` skips, so the check
/// is forced with `None` — pop's precedent. Before it, a frozen
/// `number[]`'s `push(9)` appended through the freeze and answered
/// the new length; the any-side doors had been closed one commit
/// earlier and this lane was left for last because of ownership:
///
/// At call time the raw slot value is BORROWED — the array's stake is
/// the `emit_rc_inc` that follows the call, and an owned-shape arg's
/// own stake is released after that. On the throw edge neither runs,
/// so the arg's own stake (or the fresh Str the Substr-coerce lane
/// minted) would leak; parking it on `throw_live` for the call's
/// duration hands it to the throw path's drain instead, with the same
/// predicate `release_owned_temp` uses on the normal edge.
///
/// Returns the (possibly-realloc'd) array; the caller loads the new
/// length from it exactly as before.
pub(crate) fn emit_grow_kernel_checked(
    ctx: &mut LowerCtx<'_>,
    fid: crate::ssa::FuncId,
    recv: Operand,
    slot_arg: Operand,
    arg_eid: ExprId,
    val: Operand,
    elem_ty: Type,
    val_owned_from_substr: bool,
    ret_ty: Type,
) -> ValueId {
    let token = if val_owned_from_substr {
        Some(ctx.push_throw_temp(val, elem_ty))
    } else {
        ctx.park_owned_temp(arg_eid, &val)
    };
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![recv, slot_arg]),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.unpark_owned_temp(token);
    if elem_ty.is_refcounted() && !val_owned_from_substr {
        ctx.emit_rc_inc(val);
        // Chunk 733 — an owned-shape arg hands its +1 to the array:
        // the inc above is the array's stake, so the temp's own must
        // release. Borrow shapes are a no-op through the predicate;
        // the substr-coerce lane skips (its fresh rc=1 IS the
        // hand-off).
        ctx.release_owned_temp(arg_eid, &val);
    }
    new_arr
}
