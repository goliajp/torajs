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
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_closure_emit::{ClosureThis, emit_closure_callee_with_this};

/// Try to dispatch a fn-indirect call. Returns `Some` when one of the
/// two indirect arms matched; `None` lets the caller continue to the
/// M3 retarget / direct resolve path.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(op) = try_lower_local_fnsig(ctx, eid, callee, args) {
        return Some(op);
    }
    try_lower_call_or_closure_callee(ctx, eid, callee, args)
}

/// M2 Phase B Stage 4 — `let f = global_fn; f(args);` or fn-typed param
/// dispatch. The local slot holds an SSA value whose type is
/// `Type::FnSig(sig)`; load it, build per-arg argv (with Type::Any
/// boxing where target param is Type::Any), emit `CallIndirect`,
/// `emit_throw_check`, return the result.
fn try_lower_local_fnsig(
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

/// Generalized indirect: callee is a `Call` expr (`f(0)(5)` chained),
/// a `Closure` literal (`(() => v)()` IIFE), or an `Index` read
/// (`ops[i](x)` — chunk 733, fn-typed array elements are Closure-repr
/// so the loaded element dispatches through the env-first ABI; the
/// element read is a +0 borrow, the array binding keeps the value
/// alive across the call). Lower the callee value, dispatch by SSA
/// type. Mirrors `call_fn_value` but with explicit Void return
/// handling.
fn try_lower_call_or_closure_callee(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !matches!(
        ctx.ast.get_expr(callee),
        Expr::Call { .. } | Expr::Closure { .. } | Expr::Index { .. }
    ) {
        return None;
    }
    let callee_op = ctx.lower_expr(callee);
    let callee_ty = ctx.operand_ty(&callee_op);
    match callee_ty {
        Type::Closure(user_sig_id) => {
            // Rotation 362 — a rest-tail callee VALUE (`box[0](…)` where
            // the element's checker type is `(...args: any[]) => R`)
            // never static-dispatches: the SSA sig only carries the
            // fixed prefix, so the env-first CallIndirect argv would
            // mismatch the closure's real native arity (the argv-face
            // injected params read garbage — SIGSEGV). Route through
            // the boxed dual entry, mirroring the struct-field arm
            // (`callable_field_is_variadic`).
            if matches!(
                ctx.expr_types.get(&callee),
                Some(crate::check::Type::Function(ps, _))
                    if matches!(ps.last(), Some(crate::check::Type::Rest(_)))
            ) {
                return Some(
                    crate::ssa_lower_call_closure_local::emit_variadic_boxed_call(
                        ctx,
                        callee_op,
                        user_sig_id,
                        args,
                    ),
                );
            }
            // Rotation 328 — an inline PROMOTED fn-expr callee (the
            // IIFE arm): its native ABI carries the `__this: any`
            // param the Closure sig sheds, so the receiverless call
            // must re-insert the slot with a boxed `undefined` — the
            // plain env-first shape would shift every user arg by
            // one. A runtime closure value arriving through a Call /
            // Index callee cannot be promoted (the AST bar admits
            // zero-alias faces only) and keeps the plain shape.
            let this = match ctx.ast.get_expr(callee) {
                Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name) => {
                    ClosureThis::Undef
                }
                _ => ClosureThis::None,
            };
            // 403-03 — an fn-typed boundary can hand back the
            // undefined sentinel (coerce_to_ret's Any→Closure arm);
            // calling it must be a catchable TypeError, not a jump
            // through bytes past the sentinel header. No-op unless
            // the callee source can spell the sentinel.
            crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, callee, &callee_op);
            Some(emit_closure_callee_with_this(
                ctx,
                eid,
                callee_op,
                user_sig_id,
                args,
                this,
            ))
        }
        Type::FnSig(sig_id) => Some(emit_fnsig_callee(ctx, eid, callee_op, sig_id, args)),
        _ => {
            // Non-callable — fall through to resolve_callee for the panic
            // with a clearer error message.
            None
        }
    }
}

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
    // emit_closure_callee / arm-1 P0.5).
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
