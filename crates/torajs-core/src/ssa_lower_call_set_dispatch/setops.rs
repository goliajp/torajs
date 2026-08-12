//! ES2025 setops arms of the Set method dispatch — split from
//! [`super`] (`ssa_lower_call_set_dispatch.rs`) when the §24.2.1.2
//! GetSetRecord route pushed the parent over the 500-line cap.
//! Verbatim moves; the parent's dispatch match is the only caller.

use crate::ast::ExprId;
use crate::check as check_mod;
use crate::ssa::{FuncId, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// isSubsetOf / isSupersetOf / isDisjointFrom shared shape (ES2025
/// read-only setops): a statically-Set argument keeps the Set×Set
/// fast kernel (both sides borrowed; runtime returns i64, narrowed
/// back to Bool the same way Set.has does); anything else boxes and
/// walks the §24.2.1.2 GetSetRecord protocol kernel (`setlike_op`
/// picks the algorithm), whose refusals / user-callback throws land
/// as pending state the emitted throw check propagates.
pub(super) fn emit_relation_predicate(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
    setlike_op: i64,
) -> Operand {
    debug_assert!(!args.is_empty());
    let other_op = ctx.lower_expr(args[0]);
    let statically_set = matches!(ctx.expr_types.get(&args[0]), Some(check_mod::Type::Set));
    // S318 — ES §24.2.5.{4-6} silently ignore args past index 0; mirror
    // S272 lower-and-drop trailing.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let r = if statically_set {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(intrinsic, vec![recv_op, other_op]),
            Type::I64,
            None,
        )
    } else {
        let other_any = ctx.box_to_any_from_expr(args[0], other_op.clone());
        let r = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.set_relation_setlike,
                vec![recv_op, other_any, Operand::ConstI64(setlike_op)],
            ),
            Type::I64,
            None,
        );
        // An inline set-like literal is an owned temp — the kernel
        // borrowed it; settle its mint here.
        ctx.release_owned_temp(args[0], &other_op);
        ctx.emit_throw_check(None);
        r
    };
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// union / intersection / difference / symmetricDifference shared shape
/// (ES2025 combining setops): fresh Set; receiver + other are borrowed.
/// A statically-Set argument keeps the Set×Set fast kernel (heap keys
/// are rc_inc'd by the runtime helper before re-insert so the source
/// still owns its own ref); anything else boxes and walks the
/// §24.2.1.2 GetSetRecord protocol kernel (`setlike_op` picks the
/// algorithm) with the throw check right after.
pub(super) fn emit_setop(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
    setlike_op: i64,
) -> Operand {
    debug_assert!(!args.is_empty());
    let other_op = ctx.lower_expr(args[0]);
    let statically_set = matches!(ctx.expr_types.get(&args[0]), Some(check_mod::Type::Set));
    // S318 — ES §24.2.5.{7-10} silently ignore args past index 0.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    if statically_set {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(intrinsic, vec![recv_op, other_op]),
            Type::Set,
            None,
        );
        return Operand::Value(v);
    }
    let other_any = ctx.box_to_any_from_expr(args[0], other_op.clone());
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.set_setop_setlike,
            vec![recv_op, other_any, Operand::ConstI64(setlike_op)],
        ),
        Type::Set,
        None,
    );
    // See the relation twin — settle an inline literal's mint.
    ctx.release_owned_temp(args[0], &other_op);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}
