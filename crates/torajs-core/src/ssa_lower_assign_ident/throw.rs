//! The two throwing write lanes of [`super`] — a write to an
//! unresolvable name in strict code (§6.2.5.6) and a write to a
//! fn-expression's own self-name (§15.5.5). Split out at the 500-line
//! limit (rotation 635); a child module reaches the parent's private
//! items directly, so nothing changed visibility.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

/// RFC 20260730-undeclared-ident, write position — §6.2.5.6 PutValue
/// on an unresolvable Reference (the checker marked the target eid).
/// The RHS lowers first (§13.15.2 rref before PutValue) and its value
/// drops — the throw makes it unobservable — then the ReferenceError
/// raiser fires with the same `<name> is not defined` kernel the read
/// side uses. The lane past the throw-check is unreachable;
/// `undefined`'s shape stands in so the enclosing expression still
/// types out.
/// §13.15.2 — the RHS evaluates first and its value drops (the throw
/// makes it unobservable), then the readonly-assign raiser records the
/// pending TypeError and the throw-check propagates it. `undefined`'s
/// shape stands in so the enclosing expression still types out.
pub(super) fn lower_self_name_write_throw(ctx: &mut LowerCtx<'_>, value: ExprId) -> Operand {
    let v = ctx.lower_expr(value);
    let v_ty = ctx.operand_ty(&v);
    if !v_ty.is_copy() {
        ctx.emit_drop_value(v, v_ty);
    }
    let raiser = ctx.intrinsics.throw_readonly_assign;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(raiser, vec![]));
    ctx.emit_throw_check(None);
    Operand::ConstPtrNull
}

pub(super) fn lower_undeclared_write_throw(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    value: ExprId,
    dead_zone: bool,
) -> Operand {
    let v = ctx.lower_expr(value);
    let v_ty = ctx.operand_ty(&v);
    if !v_ty.is_copy() {
        ctx.emit_drop_value(v, v_ty);
    }
    let name_str = ctx.intern_string_literal(name);
    // The rhs is evaluated either way (§13.15.2 evaluates it before
    // PutValue), so only the raiser differs.
    let raiser = if dead_zone {
        ctx.intrinsics.throw_reference_error_tdz
    } else {
        ctx.intrinsics.throw_reference_error_name
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(raiser, vec![Operand::Value(name_str)]),
    );
    ctx.emit_throw_check(None);
    Operand::ConstPtrNull
}
