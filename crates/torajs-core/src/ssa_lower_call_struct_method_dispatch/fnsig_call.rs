//! The `Type::FnSig` field arm of the struct-method dispatch: a direct
//! fn pointer stored in the slot, called with the bare signature.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::LowerCtx;

pub(super) fn emit_fnsig_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    sig_id: SigId,
    args: &[ExprId],
    pad_n: usize,
) -> Operand {
    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, recv_op, offset),
        Type::Ptr,
        None,
    );
    // The call lands in cur_block AS OF after the args are lowered —
    // a branching arg (ternary) moves cur_block to its merge block;
    // a pre-lower snapshot put the call in the terminated pre-branch
    // block with the merge block's operands (chunk 659, same family
    // as the chunk-656 regex-method fix). The fn_ptr load above
    // dominates the merge block, so the cross-block use is sound.
    let (params, ret_ty) = ctx.fn_sigs[sig_id.0 as usize].clone();
    // S3.5 — beyond-arity args evaluate for their side effects but
    // stay out of the bare-sig argv (mirror of emit_closure_call).
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let op = ctx.lower_expr(*a);
        if i >= params.len() {
            ctx.release_owned_temp(*a, &op);
            continue;
        }
        argv.push(op);
    }
    // RFC 20260714-t262-top-clusters 刀 1 — same call-boundary
    // coercion as the direct-call terminal: an Array-literal arg
    // into an any param used to pass its typed repr raw (elem reads
    // answered undefined).
    let coerce_owned = crate::ssa_lower_call_terminal_coerce::coerce_args_by_param_tys(
        ctx, &params, args, &mut argv,
    );
    crate::ssa_lower_call_terminal::pad_undef_n(ctx, pad_n, &mut argv);
    let result = if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        Operand::ConstPtrNull
    } else {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
            ret_ty,
            None,
        );
        Operand::Value(v)
    };
    // bug-327 C2.5 — indirect targets are unknown statically;
    // propagate a pending throw the same way direct user-fn calls do.
    ctx.emit_throw_check(None);
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    result
}
