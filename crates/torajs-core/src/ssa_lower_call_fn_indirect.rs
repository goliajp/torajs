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
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    let mut coerce_owned: Vec<(Operand, Type)> = Vec::new();
    let mut argv: Vec<Operand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            // Chunk 641 — empty `[]` arg allocs with the param's layout.
            let raw = ctx
                .try_lower_empty_array_arg(*a, target_params.get(i))
                .unwrap_or_else(|| ctx.lower_expr(*a));
            owned_temps.push((*a, raw));
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
                    &mut coerce_owned,
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
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        for (op, ty) in coerce_owned {
            ctx.emit_drop_value(op, ty);
        }
        return Some(Operand::ConstPtrNull);
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
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
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

/// Call-site `this` for a closure callee's leading `__this` slot.
pub(crate) enum ClosureThis {
    /// Plain env-first ABI — the callee has no `__this` slot.
    None,
    /// Explicit thisArg expression (`.call`/`.apply` face, rotation
    /// 261) — evaluated ahead of the user args, spec order.
    Expr(ExprId),
    /// Boxed `undefined` — a promoted callee at a receiverless direct
    /// call (the rotation-328 IIFE arm; §10.2.1.2 strict-mode
    /// call-site `this`).
    Undef,
}

/// Emit env-first ABI CallIndirect for a `Type::Closure` callee:
/// load fn_ptr from `CLOSURE_FN_ADDR_OFF`, prepend env_ptr to argv,
/// intern an env-first signature for the indirect call.
pub(crate) fn emit_closure_callee(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee_op: Operand,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    emit_closure_callee_with_this(ctx, eid, callee_op, user_sig_id, args, ClosureThis::None)
}

/// Rotation 261 — the inline fn-expr `.call`/`.apply` face entry:
/// [`ClosureThis::Expr`] boxes the EXPLICIT thisArg (§20.2.3.3/.1)
/// into the promoted callee's leading `__this` argv slot; the plain
/// entry above passes [`ClosureThis::None`] and keeps the env-first
/// shape byte-for-byte.
pub(crate) fn emit_closure_callee_with_this(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee_op: Operand,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
    this_arg: ClosureThis,
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
    let mut env_first = Vec::with_capacity(user_params.len() + 3);
    env_first.push(Type::Ptr);
    // S1 (RFC 20260810-indirect-argc-abi) — hidden I64 argc at ABI
    // position 1 of every `__env`-first entry.
    env_first.push(Type::I64);
    if !matches!(this_arg, ClosureThis::None) {
        env_first.push(Type::Any);
    }
    env_first.extend(user_params.iter().copied());
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, env_first, ret_ty.clone());
    // 398-06 — a ClosureThis::None callee VALUE may still be a
    // promoted closure at runtime (a field read, an alias, a slot the
    // value flowed through), so the receiverless call runs behind the
    // FLAG_CLOSURE_RECV_FIRST gate (doc on `ssa_lower_call_recv_gate`).
    // With no promoted closure anywhere the flag can never be set —
    // keep the single-path emit (knife-2 whole-program kill, doc on
    // the closure_local twin).
    let recv_gated = matches!(this_arg, ClosureThis::None) && !ctx.ast.fnexpr_recv_fns.is_empty();
    let user_params = ctx.fn_sigs[user_sig_id.0 as usize].0.clone();
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 3);
    argv.push(Operand::Value(env_ptr));
    // S1 argc slot — the real user argument count.
    argv.push(Operand::ConstI64(args.len() as i64));
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    let mut coerce_owned: Vec<(Operand, Type)> = Vec::new();
    match this_arg {
        ClosureThis::Expr(t) => {
            let raw = ctx.lower_expr(t);
            owned_temps.push((t, raw));
            let boxed = crate::ssa_lower_call_arg_conv::emit_arg_conv(
                ctx,
                Type::Any,
                t,
                raw,
                &mut coerce_owned,
            );
            argv.push(boxed);
        }
        // `undefined` NaN-box into the `__this` slot (the
        // closure_local receiverless-call mirror).
        ClosureThis::Undef => {
            let undef_box = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            argv.push(Operand::Value(undef_box));
        }
        ClosureThis::None => {}
    }
    for (i, a) in args.iter().enumerate() {
        // Chunk 641 — empty `[]` arg allocs with the param's layout.
        let raw = ctx
            .try_lower_empty_array_arg(*a, user_params.get(i))
            .unwrap_or_else(|| ctx.lower_expr(*a));
        owned_temps.push((*a, raw));
        if i >= user_params.len() {
            continue; // beyond-arity: evaluated, value unused
        }
        // RC-4 F3 — Type::Any target param boxes a concrete arg
        // (arm-1 P0.5 mirror): an untyped IIFE param is Type::Any at
        // the ABI, and passing a raw i64 into the box-shaped lane
        // made `(function(a){ return a })(1)` SIGSEGV on the
        // callee's deref (test262 asi / statements-function /
        // arguments-object family). Boxing is a pure bit-encode; the
        // raw temp's release below still settles owned temps.
        // RFC 20260707 chunk 626 — typed array into an Arr<Any>
        // param marks the block's elem kind (self-gating).
        if let Some(p) = user_params.get(i) {
            ctx.mark_arr_arg_for_any_param(*p, &raw);
        }
        // Boxing was the only conversion this lane carried; the rest
        // of the contract applies here for the same reasons.
        let converted = match user_params.get(i) {
            Some(&p) => {
                crate::ssa_lower_call_arg_conv::emit_arg_conv(ctx, p, *a, raw, &mut coerce_owned)
            }
            None => raw,
        };
        argv.push(converted);
    }
    // T-28 — missing trailing Any args pad with ANY_UNDEF (see
    // pad_trailing_undef doc; short argv = garbage-register reads).
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    // Chunk 569 — args SHARE: no consume (the historical consume
    // orphaned live-binding stakes); owned temps release after.
    let result = if !recv_gated {
        // Static receiver slot (Expr / Undef) — single-path emit.
        if ret_ty == Type::Void {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
            );
            None
        } else {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
                ret_ty,
                None,
            );
            Some(Operand::Value(v))
        }
    } else {
        // Receiverless — the runtime FLAG_CLOSURE_RECV_FIRST gate.
        crate::ssa_lower_call_recv_gate::emit_indirect_call_recv_gated(
            ctx,
            &Operand::Value(env_ptr),
            Operand::Value(fn_ptr),
            env_first_sig,
            argv,
            crate::ssa_lower_call_recv_gate::RecvSeed::Undef,
            ret_ty,
        )
    };
    ctx.emit_throw_check(None);
    for (a, op) in owned_temps {
        ctx.release_owned_temp(a, &op);
    }
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    result.unwrap_or(Operand::ConstPtrNull)
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
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    let mut coerce_owned: Vec<(Operand, Type)> = Vec::new();
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
            owned_temps.push((*a, raw));
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
                    &mut coerce_owned,
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
        ctx.emit_throw_check(None);
        for (a, op) in owned_temps {
            ctx.release_owned_temp(a, &op);
        }
        for (op, ty) in coerce_owned {
            ctx.emit_drop_value(op, ty);
        }
        return Operand::ConstPtrNull;
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
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    Operand::Value(v)
}
