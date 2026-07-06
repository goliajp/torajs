//! Fn-indirect-call dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` as chunk-13 of the
//! `Expr::Call` god-arm decomp (chunks 1-12 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries).
//!
//! Two related arms covered here:
//! 1. **M2 Phase B Stage 4** — call through a fn-typed local
//!    (`let f = global_fn; f(args);` or fn-typed param). Load the slot,
//!    look up the signature, emit `CallIndirect`. Per-arg Type::Any
//!    boxing when target param is Type::Any (closure-lift desugar
//!    defaults unannotated closure params to Type::Any).
//! 2. **Generalized indirect** — callee is itself a `Call` expr
//!    (`f(0)(5)` chained call) or an immediately-invoked closure
//!    literal (`(() => v)()` — S158 IIFE). Lower callee, dispatch by
//!    SSA type:
//!    - `Type::Closure(sig)` — env-first ABI: load fn_ptr from
//!      `CLOSURE_FN_ADDR_OFF`, prepend env_ptr to argv, intern
//!      env-first sig.
//!    - `Type::FnSig(sig)` — direct CallIndirect with the fnsig
//!      pointer.
//!    - Non-callable types fall through to the resolve_callee panic
//!      with a clearer error message (returns `None`).
//!
//! Returns `Some(result)` when callee matches one of the two indirect
//! shapes and dispatches successfully; `None` falls through to the
//! caller's resolve_callee / M3 generic retarget path (Member callees,
//! direct Ident callees, non-callable Call/Closure callee).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx, intern_fn_sig};

/// Try to dispatch a fn-indirect call. Returns `Some` when one of the
/// two indirect arms matched; `None` lets the caller continue to the
/// M3 retarget / direct resolve path.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(op) = try_lower_local_fnsig(ctx, callee, args) {
        return Some(op);
    }
    try_lower_call_or_closure_callee(ctx, callee, args)
}

/// M2 Phase B Stage 4 — `let f = global_fn; f(args);` or fn-typed param
/// dispatch. The local slot holds an SSA value whose type is
/// `Type::FnSig(sig)`; load it, build per-arg argv (with Type::Any
/// boxing where target param is Type::Any), emit `CallIndirect`,
/// `emit_throw_check`, return the result.
fn try_lower_local_fnsig(
    ctx: &mut LowerCtx<'_>,
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
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    let argv: Vec<Operand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let raw = ctx.lower_expr(*a);
            owned_temps.push((*a, raw));
            if i < target_params.len()
                && matches!(target_params[i], Type::Any)
                && !matches!(ctx.operand_ty(&raw), Type::Any)
            {
                ctx.box_to_any_from_expr(*a, raw)
            } else {
                raw
            }
        })
        .collect();
    let ret_ty = ctx.fn_sigs[sig_id.0 as usize].1;
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        return Some(Operand::ConstI64(0));
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    for (a, op) in owned_temps {
        ctx.release_owned_temp(a, &op);
    }
    Some(Operand::Value(v))
}

/// Generalized indirect: callee is a `Call` expr (`f(0)(5)` chained) or
/// a `Closure` literal (`(() => v)()` IIFE). Lower the callee value,
/// dispatch by SSA type. Mirrors `call_fn_value` but with explicit
/// Void return handling.
fn try_lower_call_or_closure_callee(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !matches!(
        ctx.ast.get_expr(callee),
        Expr::Call { .. } | Expr::Closure { .. }
    ) {
        return None;
    }
    let callee_op = ctx.lower_expr(callee);
    let callee_ty = ctx.operand_ty(&callee_op);
    match callee_ty {
        Type::Closure(user_sig_id) => Some(emit_closure_callee(ctx, callee_op, user_sig_id, args)),
        Type::FnSig(sig_id) => Some(emit_fnsig_callee(ctx, callee_op, sig_id, args)),
        _ => {
            // Non-callable — fall through to resolve_callee for the panic
            // with a clearer error message.
            None
        }
    }
}

/// Emit env-first ABI CallIndirect for a `Type::Closure` callee:
/// load fn_ptr from `CLOSURE_FN_ADDR_OFF`, prepend env_ptr to argv,
/// intern an env-first signature for the indirect call.
fn emit_closure_callee(
    ctx: &mut LowerCtx<'_>,
    callee_op: Operand,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    let env_ptr = match callee_op {
        Operand::Value(v) => v,
        _ => panic!("ssa-lower: closure callee must be an SSA value"),
    };
    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
        Type::Ptr,
        None,
    );
    let (user_params, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
    let mut env_first = Vec::with_capacity(user_params.len() + 1);
    env_first.push(Type::Ptr);
    env_first.extend(user_params);
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, env_first, ret_ty);
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
    argv.push(Operand::Value(env_ptr));
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    for a in args {
        let raw = ctx.lower_expr(*a);
        owned_temps.push((*a, raw));
        argv.push(raw);
    }
    // Chunk 569 — args SHARE: no consume (the historical consume
    // orphaned live-binding stakes); owned temps release after.
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
        );
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        return Operand::ConstI64(0);
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
    Operand::Value(v)
}

/// Emit direct CallIndirect for a `Type::FnSig` callee (no env prefix).
fn emit_fnsig_callee(
    ctx: &mut LowerCtx<'_>,
    callee_op: Operand,
    sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    let fn_ptr = match callee_op {
        Operand::Value(v) => v,
        _ => panic!("ssa-lower: fnsig callee must be an SSA value"),
    };
    let ret_ty = ctx.fn_sigs[sig_id.0 as usize].1;
    let argv: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    // Chunk 569 — args SHARE: no consume; owned temps release after.
    let owned_temps: Vec<(ExprId, Operand)> =
        args.iter().copied().zip(argv.iter().copied()).collect();
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        return Operand::ConstI64(0);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    for (a, op) in owned_temps {
        ctx.release_owned_temp(a, &op);
    }
    Operand::Value(v)
}
