//! How an env-first CLOSURE VALUE is called — the emit half of
//! [`crate::ssa_lower_call_fn_indirect`], split out in rotation 591.
//!
//! The sibling file answers "which lane does this callee take"; this
//! one answers "given a closure cell, what instructions call it":
//! load `fn_ptr` from `CLOSURE_FN_ADDR_OFF`, prepend the env pointer
//! and the hidden argc, intern the env-first signature, and decide
//! what — if anything — occupies the leading `__this` slot
//! ([`ClosureThis`]).
//!
//! Two of the three receiver shapes are STATIC and widen the native
//! signature; the third is a runtime fact about the value in the slot
//! and rides the `FLAG_CLOSURE_RECV_FIRST` gate instead
//! (`crate::ssa_lower_call_recv_gate`), leaving the ordinary emit
//! byte-identical.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx, intern_fn_sig};

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
    /// Rotation 591 — the §13.3.6.2 Reference base of an index callee
    /// (`ops[i]()` calls with `this === ops`), already lowered.
    ///
    /// Distinct from [`ClosureThis::Undef`]: the ABI stays the plain
    /// env-first shape, because whether the callee actually carries a
    /// `__this` slot is a RUNTIME fact about the value in the slot,
    /// not something the call site knows. This rides the same
    /// `FLAG_CLOSURE_RECV_FIRST` gate `None` does and only changes
    /// what the taken arm seeds — so an ordinary callee's emit is
    /// byte-identical to `None`'s and `ops[i](x)` keeps its typed
    /// `CallIndirect` at zero cost.
    Base(Operand),
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
    // Only a STATIC receiver slot widens the native signature.
    // `Base` seeds the runtime gate below and leaves the ABI alone.
    if matches!(this_arg, ClosureThis::Expr(_) | ClosureThis::Undef) {
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
    let recv_gated = matches!(this_arg, ClosureThis::None | ClosureThis::Base(_))
        && !ctx.ast.fnexpr_recv_fns.is_empty();
    let user_params = ctx.fn_sigs[user_sig_id.0 as usize].0.clone();
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 3);
    argv.push(Operand::Value(env_ptr));
    // S1 argc slot — the real user argument count.
    argv.push(Operand::ConstI64(args.len() as i64));
    let mut temps = crate::ssa_lower_call_arg_temps::ArgTemps::new();
    let mut recv_base = None;
    match this_arg {
        ClosureThis::Expr(t) => {
            let raw = ctx.lower_expr(t);
            temps.push(ctx, t, raw);
            let boxed = crate::ssa_lower_call_arg_conv::emit_arg_conv(
                ctx,
                Type::Any,
                t,
                raw,
                &mut temps.coerce,
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
        // The base is not an argument — it only seeds the gate's
        // taken arm, so nothing joins argv here.
        ClosureThis::Base(op) => recv_base = Some(op),
        ClosureThis::None => {}
    }
    for (i, a) in args.iter().enumerate() {
        // Chunk 641 — empty `[]` arg allocs with the param's layout.
        let raw = ctx
            .try_lower_empty_array_arg(*a, user_params.get(i))
            .unwrap_or_else(|| ctx.lower_expr(*a));
        temps.push(ctx, *a, raw);
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
                crate::ssa_lower_call_arg_conv::emit_arg_conv(ctx, p, *a, raw, &mut temps.coerce)
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
            match recv_base {
                Some(op) => crate::ssa_lower_call_recv_gate::RecvSeed::BoxOf(op),
                None => crate::ssa_lower_call_recv_gate::RecvSeed::Undef,
            },
            ret_ty,
        )
    };
    temps.check_and_release(ctx);
    result.unwrap_or(Operand::ConstPtrNull)
}
