//! `P3.struct-method-dispatch` — `obj.method()` on a non-class
//! `Type::Obj(sid)` struct where the named field holds a callable
//! value (`Type::FnSig` direct fn or `Type::Closure` env + fn).
//! Pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-42 of the decomp (chunks 1-41 = ...
//! + `__torajs_in_op` multi-arm).
//!
//! Class methods use the static `__cm_<C>__<M>` dispatch (receiver
//! as first arg ABI, downstream arm); this arm covers non-class
//! inline structs whose fields hold function values. The
//! pre-existing valueOf shortcut previously returned the receiver
//! as-is even when the user OVERRODE valueOf — silent wrong. We
//! pre-check the obj's static `check::Type::Struct` + field
//! `check::Type::Function(..)` shape before lowering the receiver,
//! so a miss falls through cheaply to existing arms (universal
//! methods, virtual-dispatch, etc.) without lowering it twice.
//!
//! - `Type::FnSig(sig_id)` — direct fn pointer stored at the field
//!   slot. `Load(Ptr)` the fn ptr, lower args, `CallIndirect` with
//!   the signature. `ret_ty == Void` uses `append_void` + returns
//!   `ConstI64(0)` (the catch-all dispatch convention).
//! - `Type::Closure(user_sig_id)` — closure env ptr in the field
//!   slot; fn ptr lives at `env + CLOSURE_FN_ADDR_OFF`. Widen the
//!   signature with `Type::Ptr` prepended (M2 closure path
//!   convention) and pass env as the first arg, then the user args.
//!   A rest-tail field sig (RFC 20260708-variadic chunk 3) skips the
//!   static widen and dispatches through the boxed dual entry
//!   (`emit_variadic_boxed_call`) — one field serves closures of any
//!   declared arity.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! `Member`, expr_types not a `Struct`, field absent / not a
//! `Function`, receiver not lowering to `Type::Obj(sid)`, or the
//! field's SSA type is neither `FnSig` nor `Closure`).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx, OBJ_HEADER_SIZE, intern_fn_sig};

mod field_sig;
mod fnsig_call;
use field_sig::{callable_field_is_variadic, is_face_slot_field};
use fnsig_call::emit_fnsig_call;
pub(crate) mod objlit_slot;
pub(crate) use objlit_slot::is_objlit_method_slot;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let obj = *obj;
    let name = name.clone();
    let is_variadic = callable_field_is_variadic(ctx, obj, callee, &name)?;
    let name = name.as_str();
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    // RFC 20260705 chunk 555 park protocol — a decline after lowering
    // the receiver must park the operand so the next dispatcher reuses
    // it instead of re-emitting (a side-effecting receiver like
    // `f(x++).m()` would otherwise evaluate twice; the class-instance
    // admit above lets `gen.next()` reach here, whose name is a class
    // METHOD and never a layout field, so the no-field miss below is a
    // hot decline path, not a corner).
    let Type::Obj(sid) = recv_ty else {
        ctx.redispatch_lowered = Some((obj, recv_op));
        return None;
    };
    let field_hit = ctx.struct_layouts[sid.0 as usize]
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == name)
        .map(|(i, (_, t))| (i, *t));
    let Some((field_idx, ssa_field_ty)) = field_hit else {
        ctx.redispatch_lowered = Some((obj, recv_op));
        return None;
    };
    let offset = OBJ_HEADER_SIZE + (field_idx as u64) * 8;
    // T-28 — the checker recorded the trailing-Any missing-arg count
    // for this Call ExprId; a CallIndirect whose argv is shorter than
    // its signature reads garbage registers in the callee (the probe
    // face: a missing `x: any` answered a stray Str-tagged value).
    let pad_n = ctx.arity_pad_count.get(&eid).copied().unwrap_or(0);
    // the call reads the field first (§13.3.6.1 step 1): a getter
    // installed there runs before the arguments, and an answer the
    // typed call cannot invoke is a TypeError
    if matches!(ssa_field_ty, Type::FnSig(_) | Type::Closure(_)) {
        let slot = (offset, ssa_field_ty);
        crate::ssa_lower_member_obj_field::exotic_read::guard_callee_slot(
            ctx, obj, recv_op, slot, name,
        );
    }
    match ssa_field_ty {
        Type::FnSig(sig_id) => Some(emit_fnsig_call(ctx, recv_op, offset, sig_id, args, pad_n)),
        Type::Closure(user_sig_id) if is_variadic || is_face_slot_field(ctx, obj, name) => {
            let closure_env = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Ptr, recv_op, offset),
                Type::Ptr,
                None,
            );
            let env = Operand::Value(closure_env);
            let passed = &args[..crate::ast::source_argc(ctx.ast, eid, args)];
            Some(if is_variadic {
                crate::ssa_lower_call_closure_local::emit_variadic_boxed_call(
                    ctx,
                    env,
                    user_sig_id,
                    passed,
                )
            } else {
                // the member call's receiver is the callee's `this`
                // (§13.3.6.1), which a method stored in the slot reads
                let this = ctx.box_to_any(recv_op);
                crate::ssa_lower_call_closure_local::emit_face_slot_call(
                    ctx,
                    (eid, user_sig_id),
                    env,
                    Some(this),
                    passed,
                )
            })
        }
        Type::Closure(user_sig_id) => {
            let takes_recv = is_objlit_method_slot(ctx, sid, name);
            Some(emit_closure_call(
                ctx,
                recv_op,
                offset,
                user_sig_id,
                args,
                takes_recv,
                Some(eid),
            ))
        }
        // A field typed Function that holds an Any — a class method
        // read as a value (S2.34 hands out the prototype's cell
        // boxed): see `ssa_lower_index_any_method_call::emit_any_field_call`.
        Type::Any => Some(crate::ssa_lower_index_any_method_call::emit_any_field_call(
            ctx, obj, recv_op, name, args,
        )),
        _ => {
            ctx.redispatch_lowered = Some((obj, recv_op));
            None
        }
    }
}

/// Invoke a receiver-taking closure held in a struct field — the
/// `(__env, __this, ...user)` ABI blade 1 introduced. Object-literal
/// accessors reuse it verbatim (`ssa_lower_member_obj_field`): a getter
/// is a zero-arg method, a setter a one-arg one.
pub(crate) fn emit_receiver_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    args: &[ExprId],
) -> Operand {
    if objlit_slot::slot_takes_argv(ctx, None, &recv_op, offset) {
        let ops: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
        return objlit_slot::emit_argv_slot_call(ctx, recv_op, offset, user_sig_id, &ops);
    }
    // Accessor arity is fixed (getter 0, setter 1) — never pads.
    emit_closure_call(ctx, recv_op, offset, user_sig_id, args, true, None)
}

/// Same `(__env, __this, ...user)` ABI as [`emit_receiver_closure_call`],
/// but the user args are ALREADY lowered. `Object.assign`'s setter sink
/// (RFC 20260714-objlit-accessor blade 8) hands over a value it read
/// out of the SOURCE — there is no arg ExprId to lower, and no call-
/// boundary coercion to run: the checker matched the source's property
/// type against this setter's param type.
pub(crate) fn emit_receiver_closure_call_ops(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    arg_ops: Vec<Operand>,
) -> Operand {
    if objlit_slot::slot_takes_argv(ctx, None, &recv_op, offset) {
        return objlit_slot::emit_argv_slot_call(ctx, recv_op, offset, user_sig_id, &arg_ops);
    }
    emit_closure_call_ops(ctx, recv_op, offset, user_sig_id, arg_ops, true)
}

/// [`emit_receiver_closure_call_ops`] with the receiver slot under
/// caller control — an object-literal method that never mentions
/// `this` keeps the plain `(__env, ...user)` ABI, so seeding the
/// slot would shift every user argument by one.
pub(crate) fn emit_closure_call_ops(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    arg_ops: Vec<Operand>,
    takes_recv: bool,
) -> Operand {
    let mut site =
        prepare_closure_call(ctx, recv_op, offset, user_sig_id, takes_recv, arg_ops.len());
    site.argv.extend(arg_ops);
    finish_closure_call(ctx, site)
}

/// The env / fn-ptr loads and the widened ABI signature a closure call needs, hoisted so
/// both arg lanes (ExprId — which must lower its args BETWEEN the loads and the call — and
/// pre-lowered Operand) share one definition of the ABI.
struct ClosureCallSite {
    fn_ptr: Operand,
    user_params: Vec<Type>,
    ret_ty: Type,
    env_first_sig: SigId,
    /// Seeded with the fixed leading slots (env, and `__this` when the
    /// callee takes a receiver); the caller appends the user args.
    argv: Vec<Operand>,
    /// 398-06 — with no static receiver slot (`takes_recv` false) the field may still
    /// HOLD a promoted closure at runtime, so the call runs behind the
    /// FLAG_CLOSURE_RECV_FIRST gate: the closure env cell (for the header read) plus the
    /// method receiver (the §13.3.6.2 seed the taken arm boxes).
    recv_gate: Option<(Operand, Operand)>,
}

fn prepare_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    takes_recv: bool,
    user_argc: usize,
) -> ClosureCallSite {
    let closure_env = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, recv_op, offset),
        Type::Ptr,
        None,
    );
    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(closure_env), CLOSURE_FN_ADDR_OFF),
        Type::Ptr,
        None,
    );
    // The sig holds the USER params only. An object-literal method's
    // lifted fn really is `(__env, __this, ...user)`, and this is the
    // one arm that knows it — the receiver is deliberately absent from
    // every Type so a `__mth(` slot can live in its own struct's layout
    // without the layout naming itself (see `ast/objlit_nominal.rs`).
    let (user_params, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
    let fixed = 2 + usize::from(takes_recv);
    let mut abi_params = Vec::with_capacity(user_params.len() + fixed);
    abi_params.push(Type::Ptr);
    // S1 (RFC 20260810-indirect-argc-abi) — hidden I64 argc at ABI
    // position 1 of every `__env`-first entry.
    abi_params.push(Type::I64);
    if takes_recv {
        abi_params.push(Type::Ptr);
    }
    abi_params.extend(user_params.iter().copied());
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, abi_params, ret_ty);
    // With no promoted closure anywhere the flag can never be set —
    // keep the single-path emit (398-06 knife-2 whole-program kill,
    // doc on the closure_local twin).
    let recv_gate = if takes_recv || ctx.ast.fnexpr_recv_fns.is_empty() {
        None
    } else {
        Some((Operand::Value(closure_env), recv_op))
    };
    let mut argv: Vec<Operand> = Vec::with_capacity(user_params.len() + fixed);
    argv.push(Operand::Value(closure_env));
    argv.push(Operand::ConstI64(user_argc as i64));
    if takes_recv {
        // Borrowed, like a class method's receiver — a method does not
        // consume its `this`.
        argv.push(recv_op);
    }
    ClosureCallSite {
        fn_ptr: Operand::Value(fn_ptr),
        user_params,
        ret_ty,
        env_first_sig,
        argv,
        recv_gate,
    }
}

fn finish_closure_call(ctx: &mut LowerCtx<'_>, site: ClosureCallSite) -> Operand {
    let ClosureCallSite {
        fn_ptr,
        ret_ty,
        env_first_sig,
        argv,
        recv_gate,
        ..
    } = site;
    let result = match recv_gate {
        // 398-06 — runtime-fact receiver: gate on the closure header,
        // seeding the METHOD RECEIVER (§13.3.6.2) on the taken arm.
        Some((cell, recv_op)) => crate::ssa_lower_call_recv_gate::emit_indirect_call_recv_gated(
            ctx,
            &cell,
            fn_ptr,
            env_first_sig,
            argv,
            crate::ssa_lower_call_recv_gate::RecvSeed::BoxOf(recv_op),
            ret_ty,
        )
        .unwrap_or(Operand::ConstPtrNull),
        None => {
            if ret_ty == Type::Void {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::CallIndirect(env_first_sig, fn_ptr, argv),
                );
                Operand::ConstPtrNull
            } else {
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::CallIndirect(env_first_sig, fn_ptr, argv),
                    ret_ty,
                    None,
                );
                Operand::Value(v)
            }
        }
    };
    // bug-327 C2.5 — indirect targets are unknown statically;
    // propagate a pending throw the same way direct user-fn calls do.
    ctx.emit_throw_check(None);
    result
}

fn emit_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    args: &[ExprId],
    takes_recv: bool,
    call: Option<ExprId>,
) -> Operand {
    let argc = call.map_or(args.len(), |c| crate::ast::source_argc(ctx.ast, c, args));
    let mut site = prepare_closure_call(ctx, recv_op, offset, user_sig_id, takes_recv, argc);
    let fixed = site.argv.len();
    // Same post-lower cur_block rule as `fnsig_call` — a
    // branching arg moves cur_block; the loads dominate the merge.
    // S3.5 (RFC 20260810-indirect-argc-abi) — beyond-arity args still
    // lower (ES §13.3.6.1 side effects) but never enter argv; the S1
    // argc slot already carries the real count. A fresh-owned extra
    // releases right away — no callee ever borrows it.
    let declared = site.user_params.len();
    for (i, a) in args.iter().enumerate() {
        let op = ctx.lower_expr(*a);
        if i >= declared {
            ctx.release_owned_temp(*a, &op);
            continue;
        }
        site.argv.push(op);
    }
    // RFC 20260714-t262-top-clusters 刀 1 — same call-boundary
    // coercion as the direct-call terminal (the leading env / `__this`
    // slots are ours; the user args align with the user param list).
    let user_params = site.user_params.clone();
    let coerce_owned = crate::ssa_lower_call_terminal_coerce::coerce_args_by_param_tys(
        ctx,
        &user_params,
        args,
        &mut site.argv[fixed..],
    );
    let pad_n = call.and_then(|c| ctx.arity_pad_count.get(&c).copied());
    crate::ssa_lower_call_terminal::pad_undef_n(ctx, pad_n.unwrap_or(0), &mut site.argv);
    let result = finish_closure_call(ctx, site);
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    result
}
