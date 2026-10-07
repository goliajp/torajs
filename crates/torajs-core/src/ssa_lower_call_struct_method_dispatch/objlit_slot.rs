//! What an object-literal struct slot holds, asked by the field-call
//! and accessor lanes before they pick a calling convention.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260714-objlit-accessor blade 1 — is `(sid, name)` an
/// object-literal METHOD slot (`{ m() { ... } }`) rather than a plain
/// fn-valued field (`{ f: () => ... }`)? A method's lifted closure
/// takes the receiver as `__this`, so the call has to push it.
///
/// Resolved through the synthetic NOMINAL alias `desugar_objlit_nominal`
/// minted for the literal — never by matching struct shapes. The class
/// accessor lane answers the same question by scanning `aliases` for a
/// structurally-equal entry, and that is exactly why a plain `{a:1}`
/// steals a same-layout class's getter (RFC §2.1); this must not copy it.
pub(crate) fn is_objlit_method_slot(
    ctx: &LowerCtx<'_>,
    sid: crate::ssa::StructId,
    name: &str,
) -> bool {
    ctx.ast
        .objlit_method_fields
        .iter()
        .filter(|(_, methods)| methods.iter().any(|m| m == name))
        .any(|(objlit_ty, _)| matches!(ctx.aliases.get(objlit_ty), Some(Type::Obj(s)) if *s == sid))
}

/// Is the accessor slot at `offset` of the receiver's layout held by a
/// closure on the argv face? Its native entry carries the raw argv
/// pointer, which only the boxed dual entry fills.
///
/// A layout is matched by field names: literals with the same names can
/// share it (the value layout is interned apart from any nominal alias,
/// and the closure types in it do not tell the two faces apart), and a
/// `__getter_` / `__setter_` name only ever comes from an accessor
/// literal. A receiver bound to one literal (`recv`, when the lane has
/// the expression) names the closure in the slot, so the answer is that
/// closure's; any other receiver sharing an argv slot's names takes the
/// boxed route, which serves every accessor closure.
pub(crate) fn slot_takes_argv(
    ctx: &LowerCtx<'_>,
    recv: Option<ExprId>,
    recv_op: &Operand,
    offset: u64,
) -> bool {
    let slots = &ctx.ast.objlit_argv_slots;
    if slots.rows.is_empty() {
        return false;
    }
    let Type::Obj(sid) = ctx.operand_ty(recv_op) else {
        return false;
    };
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let idx = ((offset - crate::ssa_lower::OBJ_HEADER_SIZE) / 8) as usize;
    let Some((slot, _)) = layout.get(idx) else {
        return false;
    };
    let shared = slots.rows.iter().any(|(names, f, _)| {
        *slot == f.as_str()
            && names.len() == layout.len()
            && names.iter().zip(layout).all(|(n, (k, _))| *k == n.as_str())
    });
    let lit = recv.and_then(|r| match ctx.ast.get_expr(r) {
        Expr::ObjectLit { .. } => Some(r),
        Expr::Ident(n) => slots.bound.get(n).copied(),
        _ => None,
    });
    let held = lit.and_then(|l| match ctx.ast.get_expr(l) {
        Expr::ObjectLit { fields } => fields.iter().find(|(k, _)| k == slot).map(|(_, v)| *v),
        _ => None,
    });
    match held.map(|v| ctx.ast.get_expr(v)) {
        Some(Expr::Closure { fn_name, .. }) if shared => ctx.ast.closure_argv_fns.contains(fn_name),
        _ => shared,
    }
}

/// [[Get]] through an object-literal getter slot: no arguments
/// (§10.1.8.1 OrdinaryGet). `recv` as in [`slot_takes_argv`].
pub(crate) fn emit_getter_call(
    ctx: &mut LowerCtx<'_>,
    recv: Option<ExprId>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
) -> Operand {
    if slot_takes_argv(ctx, recv, &recv_op, offset) {
        return emit_argv_slot_call(ctx, recv_op, offset, user_sig_id, &[]);
    }
    super::emit_closure_call(ctx, recv_op, offset, user_sig_id, &[], true, None)
}

/// [[Set]] through an object-literal setter slot with the one value `op`
/// of `value` (§10.1.9.2 OrdinarySetWithOwnDescriptor). On the argv face
/// it goes in boxed; otherwise it is coerced to the setter's param, and
/// what the coercion minted is released after the call.
pub(crate) fn emit_setter_call(
    ctx: &mut LowerCtx<'_>,
    recv: Option<ExprId>,
    recv_op: Operand,
    (offset, user_sig_id): (u64, SigId),
    (op, value): (Operand, ExprId),
) {
    if slot_takes_argv(ctx, recv, &recv_op, offset) {
        emit_argv_slot_call(ctx, recv_op, offset, user_sig_id, &[op]);
        return;
    }
    let user_params = ctx.fn_sigs[user_sig_id.0 as usize].0.clone();
    let mut argv = [op];
    let coerce_owned = crate::ssa_lower_call_terminal_coerce::coerce_args_by_param_tys(
        ctx,
        &user_params,
        std::slice::from_ref(&value),
        &mut argv,
    );
    super::emit_closure_call_ops(ctx, recv_op, offset, user_sig_id, argv.to_vec(), true);
    for (o, ty) in coerce_owned {
        ctx.emit_drop_value(o, ty);
    }
}

/// Call the closure in an argv-face slot through its boxed dual entry,
/// receiver first: the arguments object then sees exactly `args` —
/// none for [[Get]] (§10.1.8.1 OrdinaryGet), the one value for [[Set]]
/// (§10.1.9.2 OrdinarySetWithOwnDescriptor). Receiver and values are
/// borrowed; the result comes back as the slot's declared return.
pub(crate) fn emit_argv_slot_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    args: &[Operand],
) -> Operand {
    let env = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, recv_op, offset),
        Type::Ptr,
        None,
    );
    let this = ctx.box_to_any(recv_op);
    let packed: Vec<(Operand, Option<ExprId>)> = args.iter().map(|&op| (op, None)).collect();
    let Operand::Value(argv) =
        crate::ssa_lower_member_obj_field::accessor_argv::pack_argv(ctx, &packed)
    else {
        unreachable!("pack_argv answers its buffer")
    };
    crate::ssa_lower_call_closure_local::emit_variadic_call_conv(
        ctx,
        Operand::Value(env),
        Some(this),
        crate::ssa_lower_any_argv::AnyArgv::borrowed(argv),
        args.len() as i64,
        user_sig_id,
    )
}
