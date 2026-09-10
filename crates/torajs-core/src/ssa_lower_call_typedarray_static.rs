//! `<TypedArray ctor>.from(source, mapfn?, thisArg?)` and
//! `<TypedArray ctor>.of(...items)` on a TYPED constructor ident —
//! §23.2.2.1 / §23.2.2.2.
//!
//! Both statics are %TypedArray%'s, and their kernels live on the
//! any lane (`torajs-anyvalue::typed_array_from`, reached through the
//! ctor-static dispatch off the interned constructor cell — the same
//! path a `: any` binding's `TA.from(…)` takes). A typed spelling
//! therefore boxes the constructor VALUE — the interned cell
//! `x === Uint8Array` compares — and rides the ordinary any-method
//! call, which hands that cell in as the receiver the recv-first
//! `from` / `of` cells read their `C` from. Arguments box per the
//! any-argv ledger (a this-reading mapfn is promoted by the
//! `fnexpr_this` from-face and gets `thisArg` on its receiver
//! channel); the answer is the fresh view, `any` the way every
//! runtime-minted buffer cell is (`fromBase64` next door).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "from" && name != "of" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if crate::ssa_lower_call_typedarray::bytes_per_element(ns).is_none()
        || ctx.ast.class_parents.contains_key(ns)
        || ctx.locals.contains_key(ns)
    {
        return None;
    }
    let tag = crate::ssa_lower_member_builtin_namespace::proto_method_tag(ns)?;
    let name = name.clone();
    // The interned ctor cell is immortal — a borrow with nothing to
    // release (`try_builtin_ctor_ident`'s shape).
    let recv = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_ctor_value,
            vec![Operand::ConstI64(tag)],
        ),
        Type::Any,
        None,
    );
    let result = crate::ssa_lower_any_method_call::emit_any_method_call(
        ctx,
        Operand::Value(recv),
        &name,
        Operand::ConstPtrNull,
        args,
    );
    // The kernel records a TypeError for a non-callable mapfn / a
    // wrong-kind element, and whatever the source's iteration
    // throws; the answer is an owned Any the propagate edge must
    // release.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Some(Operand::Value(result))
}
