//! Closure-typed local indirect-call dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` as chunk-16 of the
//! `Expr::Call` god-arm decomp (chunks 1-15 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods).
//!
//! M2 closure-local arm — `let f = makeClosure(); f(args);` where the
//! local's recorded type is `Type::Closure(user_sig_id)`. Load env_ptr
//! from the slot (it IS the env), load fn_ptr from `CLOSURE_FN_ADDR_OFF`,
//! intern an env-first sig (Ptr prepended to user-facing params), prepend
//! env_ptr to argv, emit `CallIndirect`. Type::Any target params box
//! concrete args (P0.5 — closure-lift desugar defaults unannotated
//! closure params to Type::Any). Args SHARE (chunk 569): the callee
//! body never drops its params (+0 borrow, same body contract as
//! direct calls), so the historical caller-side rc_inc had no
//! collector and leaked one ref per call; owned-shape temps release
//! after the call instead (call_terminal mirror).
//!
//! Returns `Some(result)` when callee resolves to a `Type::Closure`-typed
//! local; `None` falls through to the caller's
//! [`crate::ssa_lower_call_fn_indirect`] dispatch / `resolve_callee` path.
//! The companion `ssa_lower_call_fn_indirect` handles `Type::FnSig`-typed
//! locals + generalized Call/Closure callees; this sibling handles only
//! the bare-ident Closure local arm because the caller's ordering put it
//! ahead of the fn_indirect dispatch.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx, intern_fn_sig};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let info = ctx.locals.get(callee_name).copied()?;
    let Type::Closure(user_sig_id) = info.ty else {
        return None;
    };

    let env_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
        Type::Ptr,
        None,
    );
    // Prepend Type::Ptr to the user-facing param list to get the
    // underlying signature with the env first.
    let (user_params, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
    let mut env_first_params = Vec::with_capacity(user_params.len() + 1);
    env_first_params.push(Type::Ptr);
    env_first_params.extend(user_params.iter().copied());
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, env_first_params, ret_ty);

    // P0.5 mirror — Type::Any param boxes the concrete arg.
    // S126-3 see direct fn-call P0.9 in ssa_lower.
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
    argv.push(Operand::Value(env_ptr));
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let raw = ctx.lower_expr(*a);
        // Chunk 569 — snapshot pre-box owned temps for the
        // post-call release; borrow-shape args pass +0 and keep
        // their stake (no inc: the body never drops params, no
        // consume: TS args share).
        owned_temps.push((*a, raw));
        let boxed = if i < user_params.len()
            && matches!(user_params[i], Type::Any)
            && !matches!(ctx.operand_ty(&raw), Type::Any)
        {
            ctx.box_to_any_from_expr(*a, raw)
        } else {
            raw
        };
        argv.push(boxed);
    }
    // T-28 — missing trailing Any args pad with ANY_UNDEF (checker
    // recorded arity_pad_count); without the pad the CallIndirect
    // argv is shorter than env_first_sig and the callee reads
    // garbage registers (RC-4 arguments-object SIGSEGV).
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
        );
        // bug-327 C2.5 — indirect targets are unknown statically;
        // propagate a pending throw the same way direct user-fn calls do.
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        return Some(Operand::ConstI64(0));
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    for (a, op) in owned_temps {
        ctx.release_owned_temp(a, &op);
    }
    Some(Operand::Value(v))
}
