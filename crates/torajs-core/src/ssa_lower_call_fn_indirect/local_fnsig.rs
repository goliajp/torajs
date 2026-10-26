//! The `Type::FnSig` local arm of [`super::try_lower`]: a binding or
//! parameter holding a bare code address.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// M2 Phase B Stage 4 — `let f = global_fn; f(args);` or fn-typed param
/// dispatch. The local slot holds an SSA value whose type is
/// `Type::FnSig(sig)`; load it, build per-arg argv (with Type::Any
/// boxing where target param is Type::Any), emit `CallIndirect`,
/// `emit_throw_check`, return the result.
pub(super) fn try_lower_local_fnsig(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let info = ctx.locals.get(callee_name).copied()?;
    let Type::FnSig(sig_id) = info.ty else {
        return None;
    };
    // an `any` argument for an object parameter: a `const` binding of a
    // declared function is retargeted to that function's widened clone
    // (`check_type_of_call::any_struct_slot`), which the direct-call lane
    // takes; any other bare pointer has no boxed entry to hand it to, and
    // the checker refuses the call
    if crate::ssa_lower_call_closure_local::any_into_obj_slot(ctx, args, sig_id) {
        assert!(
            ctx.call_retargets.contains_key(&eid),
            "ssa-lower: `{callee_name}` holds a bare function pointer and takes an `any` \
             argument for an object parameter"
        );
        return None;
    }

    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    // P0.5 — Type::Any target param boxes concrete arg (closure-lift
    // desugar defaults unannotated closure params to Type::Any).
    // S126-3 see direct fn-call P0.9 below.
    let target_params = ctx.fn_sigs[sig_id.0 as usize].0.clone();
    // Chunk 569 — args SHARE: +0 borrows, no inc (the body never
    // drops its params — the historical rc_inc leaked one ref per
    // call, probe-proven), no consume; owned-shape temps release
    // after the call (call_terminal mirror).
    let mut temps = crate::ssa_lower_call_arg_temps::ArgTemps::new();
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
    // Beyond-arity extras lowered above for §13.3.6.1 side effects;
    // the CallIndirect signature reads only its declared slots.
    if argv.len() > target_params.len() {
        argv.truncate(target_params.len());
    }
    // T-28 — missing trailing Any args pad with ANY_UNDEF (see
    // pad_trailing_undef doc; short argv = garbage-register reads).
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    let ret_ty = ctx.fn_sigs[sig_id.0 as usize].1;
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        temps.check_and_release(ctx);
        return Some(Operand::ConstPtrNull);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    temps.check_and_release(ctx);
    Some(Operand::Value(v))
}
