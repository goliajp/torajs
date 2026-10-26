//! The `Type::FnSig` callee arm of [`super::try_lower`]: a direct
//! CallIndirect through a code address, no env prefix.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Emit direct CallIndirect for a `Type::FnSig` callee (no env prefix).
pub(crate) fn emit_fnsig_callee(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee_op: Operand,
    sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    let fn_ptr = match callee_op {
        Operand::Value(v) => v,
        _ => panic!("ssa-lower: fnsig callee must be an SSA value"),
    };
    let ret_ty = ctx.fn_sigs[sig_id.0 as usize].1;
    let target_params = ctx.fn_sigs[sig_id.0 as usize].0.clone();
    // Chunk 569 — args SHARE: no consume; owned temps release after.
    let mut temps = crate::ssa_lower_call_arg_temps::ArgTemps::new();
    // RC-4 F3 — Type::Any target param boxes a concrete arg (see
    // emit_closure_callee_with_this / arm-1 P0.5).
    let mut argv: Vec<Operand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            // Chunk 641 — empty `[]` arg allocs with the param's layout.
            let raw = ctx
                .try_lower_empty_array_arg(*a, target_params.get(i))
                .unwrap_or_else(|| ctx.lower_expr(*a));
            temps.push(ctx, *a, raw);
            // RFC 20260707 chunk 626 — typed array into an Arr<Any>
            // param marks the block's elem kind (self-gating).
            if let Some(p) = target_params.get(i) {
                ctx.mark_arr_arg_for_any_param(*p, &raw);
            }
            // Boxing was the only conversion this lane carried; the
            // rest of the contract applies here for the same reasons.
            match target_params.get(i) {
                Some(&p) => crate::ssa_lower_call_arg_conv::emit_arg_conv(
                    ctx,
                    p,
                    *a,
                    raw,
                    &mut temps.coerce,
                ),
                None => raw,
            }
        })
        .collect();
    // T-28 — missing trailing Any args pad with ANY_UNDEF (see
    // pad_trailing_undef doc; short argv = garbage-register reads).
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        temps.check_and_release(ctx);
        return Operand::ConstPtrNull;
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    temps.check_and_release(ctx);
    Operand::Value(v)
}
