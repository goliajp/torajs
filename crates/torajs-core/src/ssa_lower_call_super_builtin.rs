//! `__superbuiltin__<m>(this, args…)` — the desugared `super.m()`
//! of a builtin-heritage subclass method (rotation 371;
//! desugar_classes_super routes here when no user ancestor declares
//! `m` and the heritage chain roots in a builtin). Lowers to the
//! runtime super re-dispatch: the method resolves on the parent
//! PROTOTYPE per §13.3.7.3, so the receiver's own override is
//! skipped — `__torajs_super_builtin_method` enters the builtin
//! dispatch arms directly. The kernel resolves by name off
//! `fn_table` (the `__torajs_array_from_dyn` precedent — no
//! Intrinsics struct slot).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower a super-builtin call. `Some` when the callee is the
/// desugar's `__superbuiltin__` ident.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let m_name = match ctx.ast.get_expr(callee) {
        Expr::Ident(n) => n.strip_prefix("__superbuiltin__")?.to_string(),
        _ => return None,
    };
    let fid = *ctx.fn_table.get("__torajs_super_builtin_method")?;
    // args[0] is the desugar's `this` (the subclass receiver);
    // box at the any-lane boundary (borrow-shaped, the kernel
    // borrows the receiver).
    let recv_raw = ctx.lower_expr(args[0]);
    let recv = if matches!(ctx.operand_ty(&recv_raw), Type::Any) {
        recv_raw.clone()
    } else {
        ctx.box_to_any(recv_raw.clone())
    };
    // A name outside the builtin id space passes mid 0 — the kernel
    // answers the spec not-a-function TypeError.
    let mid = torajs_rc::any_method_id(&m_name);
    let (argv, boxed_slots) = crate::ssa_lower_any_method_call::pack_any_argv(ctx, &args[1..]);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            fid,
            vec![
                recv,
                Operand::ConstI64(mid),
                Operand::Value(argv),
                Operand::ConstI64((args.len() - 1) as i64),
            ],
        ),
        Type::Any,
        None,
    );
    // Release the argv boxes before the throw check (any_method_call
    // mirror — the runtime borrowed argv).
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Some(Operand::Value(result))
}
