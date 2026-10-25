//! `x instanceof Iterator`: §7.3.22 membership on the prototype chain,
//! which no heap tag records. Moved out of the parent whole.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `x instanceof Iterator` — see the call site's rationale. The
/// runtime walk is `__torajs_instanceof_builtin_proto(v, 15)` (15 =
/// %Iterator.prototype%'s builtin-proto tag).
pub(super) fn lower_iterator_instanceof(
    ctx: &mut LowerCtx<'_>,
    v: Operand,
    actual_ty: Type,
) -> Operand {
    const ITERATOR_PROTO_TAG: i64 = 15;
    let v_any = match actual_ty {
        Type::MapIter | Type::ArrIter => return Operand::ConstBool(true),
        Type::Any => v,
        Type::Obj(_) => ctx.box_to_any(v),
        _ => return Operand::ConstBool(false),
    };
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.instanceof_builtin_proto,
            vec![v_any, Operand::ConstI64(ITERATOR_PROTO_TAG)],
        ),
        Type::Bool,
        None,
    );
    // a Proxy's `getPrototypeOf` trap runs on the walk
    ctx.emit_throw_check(None);
    Operand::Value(r)
}
