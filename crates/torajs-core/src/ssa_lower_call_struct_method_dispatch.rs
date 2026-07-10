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
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, SigId, Type};
use crate::ssa_lower::{CLOSURE_FN_ADDR_OFF, LowerCtx, OBJ_HEADER_SIZE, intern_fn_sig};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let obj = *obj;
    let check_mod::Type::Struct(fields) = ctx.expr_types.get(&obj)? else {
        return None;
    };
    let (_, field_check_ty) = fields.iter().find(|(n, _)| n == name)?;
    // RFC 20260710 C5 — an optional fn field (`cb?: (n) => R`) is
    // declared Nullable(Function); the checker only admits the call
    // inside a truthiness-narrowed branch (`if (o.cb) { o.cb() }`),
    // so by here the callable shape is guaranteed — unwrap to reach
    // the signature. Un-narrowed calls never lower (loud reject).
    let field_check_ty = match field_check_ty {
        check_mod::Type::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    let check_mod::Type::Function(field_params, _) = field_check_ty else {
        return None;
    };
    // RFC 20260708-variadic chunk 3 — a rest-tail field sig
    // (`fn: (...args: E[]) => R`) never static-dispatches: the SSA
    // sig only carries the fixed prefix, so the CallIndirect argv
    // would mismatch the stored closure's real arity (SIGSEGV).
    // Route through the boxed dual entry instead.
    let is_variadic = matches!(field_params.last(), Some(check_mod::Type::Rest(_)));
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    let Type::Obj(sid) = recv_ty else {
        return None;
    };
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let (field_idx, ssa_field_ty) = layout
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == name)
        .map(|(i, (_, t))| (i, *t))?;
    let offset = OBJ_HEADER_SIZE + (field_idx as u64) * 8;
    match ssa_field_ty {
        Type::FnSig(sig_id) => Some(emit_fnsig_call(ctx, recv_op, offset, sig_id, args)),
        Type::Closure(user_sig_id) if is_variadic => {
            let closure_env = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Ptr, recv_op, offset),
                Type::Ptr,
                None,
            );
            Some(
                crate::ssa_lower_call_closure_local::emit_variadic_boxed_call(
                    ctx,
                    Operand::Value(closure_env),
                    user_sig_id,
                    args,
                ),
            )
        }
        Type::Closure(user_sig_id) => {
            Some(emit_closure_call(ctx, recv_op, offset, user_sig_id, args))
        }
        _ => None,
    }
}

fn emit_fnsig_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    sig_id: SigId,
    args: &[ExprId],
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
    let argv: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    let (_params, ret_ty) = ctx.fn_sigs[sig_id.0 as usize].clone();
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        );
        return Operand::ConstI64(0);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    Operand::Value(v)
}

fn emit_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    args: &[ExprId],
) -> Operand {
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
    let (user_params, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
    let mut env_first_params = Vec::with_capacity(user_params.len() + 1);
    env_first_params.push(Type::Ptr);
    env_first_params.extend(user_params);
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, env_first_params, ret_ty);
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
    argv.push(Operand::Value(closure_env));
    // Same post-lower cur_block rule as emit_fnsig_call above — a
    // branching arg moves cur_block; the loads dominate the merge.
    for a in args {
        argv.push(ctx.lower_expr(*a));
    }
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
        );
        return Operand::ConstI64(0);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    );
    Operand::Value(v)
}
