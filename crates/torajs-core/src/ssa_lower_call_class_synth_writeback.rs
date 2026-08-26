//! The `__class_<C>` / `__proto_<C>` binding writeback after a
//! register / define call — split from
//! [`crate::ssa_lower_call_class_synth_reify`] (file-size limit): the
//! reify arms answer "which own entry lands where", this answers "how
//! the module binding follows the cell the kernel may have moved".

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// rotation 186 — refresh the `__class_<C>` / `__proto_<C>`
/// top-level binding after a define call: a dynobj define may
/// RESIZE (fresh block + free old), and while the kernel writes the
/// by-tag table back, the module binding's slot still holds the old
/// pointer — a later top-level read would dereference freed memory.
/// Reads the table's current bits (borrow, no rc traffic) and
/// stores them over the slot. No-op when the binding isn't a local
/// in the current lowering context (fn bodies read the class
/// through `class_get`, which consults the table directly).
pub(crate) fn emit_class_binding_writeback(
    ctx: &mut LowerCtx<'_>,
    cname: &str,
    tag: u32,
    is_proto: bool,
) {
    let gname = if is_proto {
        format!("__proto_{cname}")
    } else {
        format!("__class_{cname}")
    };
    let Some(info) = ctx.locals.get(&gname).copied() else {
        return;
    };
    if !matches!(info.ty, Type::Any) {
        return;
    }
    let raw_fid = if is_proto {
        ctx.intrinsics.proto_cell_raw
    } else {
        ctx.intrinsics.class_cell_raw
    };
    let raw = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(raw_fid, vec![Operand::ConstI64(tag as i64)]),
        Type::I64,
        None,
    );
    // 0 = the tag never registered (the kernel's own contract):
    // keep the binding the program already holds. Today every class
    // registers, so the branch is never taken the other way; it is
    // what makes the link-judged `class_register` elision (RFC
    // 20260824-s2-5 刀 4 A8) exact — with the call assumed away the
    // class slot stays 0 and the binding must keep the cell it
    // minted. (A `Select` is not available here: select formation
    // introduces it only after the egraph pass.)
    let registered = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(raw), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let store_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(registered),
            then_blk: store_blk,
            else_blk: after,
        },
    );
    let p = ctx.f.append_inst(
        store_blk,
        InstKind::IntToPtr(Operand::Value(raw)),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        store_blk,
        InstKind::Store(Operand::Value(p), Operand::Value(info.slot), 0),
    );
    ctx.f.set_term(store_blk, Terminator::Br(after));
    ctx.cur_block = after;
}
