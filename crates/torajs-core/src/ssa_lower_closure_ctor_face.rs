//! RFC 20260917-plain-fn-new-target knife 2 — register a function
//! expression's construct face when its closure cell is minted.
//!
//! The face (`check_monomorph_ctor_face.rs`) has no cell of its own; the
//! construct kernel finds it by the minted cell's boxed entry and enters
//! it with that cell as env. The registration is idempotent, and only a
//! closure whose face exists emits it.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// After the mint stored `boxed_op` into the cell for lifted closure
/// `fn_name`.
pub(crate) fn emit_register(ctx: &mut LowerCtx<'_>, fn_name: &str, boxed_op: &Operand) {
    if !matches!(boxed_op, Operand::Value(_)) {
        return;
    }
    let face = crate::check_monomorph_ctor_face::face_name(fn_name);
    let Some(&face_fid) = ctx.fn_table.get(&face) else {
        return;
    };
    let Some(&(adapter, sig)) = ctx.boxed_entries.get(&face_fid) else {
        return;
    };
    let cur_block = ctx.cur_block;
    let face_addr = ctx.f.append_inst(
        cur_block,
        InstKind::BoxedEntryAddr(adapter),
        Type::FnSig(sig),
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.new_target_register_entries,
            vec![boxed_op.clone(), Operand::Value(face_addr)],
        ),
    );
}
