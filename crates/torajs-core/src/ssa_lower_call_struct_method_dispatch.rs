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
    eid: ExprId,
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
    // T-28 — the checker recorded the trailing-Any missing-arg count
    // for this Call ExprId; a CallIndirect whose argv is shorter than
    // its signature reads garbage registers in the callee (the probe
    // face: a missing `x: any` answered a stray Str-tagged value).
    let pad_n = ctx.arity_pad_count.get(&eid).copied().unwrap_or(0);
    match ssa_field_ty {
        Type::FnSig(sig_id) => Some(emit_fnsig_call(ctx, recv_op, offset, sig_id, args, pad_n)),
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
            let takes_recv = is_objlit_method_slot(ctx, sid, name);
            Some(emit_closure_call(
                ctx,
                recv_op,
                offset,
                user_sig_id,
                args,
                takes_recv,
                pad_n,
            ))
        }
        _ => None,
    }
}

/// RFC 20260714-objlit-accessor blade 1 — is `(sid, name)` an
/// object-literal METHOD slot (`{ m() { ... } }`) rather than a plain
/// fn-valued field (`{ f: () => ... }`)? A method's lifted closure
/// takes the receiver as `__this`, so the call has to push it.
///
/// Resolved through the synthetic NOMINAL alias `desugar_objlit_nominal`
/// minted for the literal — never by matching struct shapes. The class
/// accessor lane answers the same question by scanning `aliases` for a
/// structurally-equal entry, and that is exactly why a plain `{a:1}`
/// steals a same-layout class's getter (RFC §2.1); this must not copy it.
fn is_objlit_method_slot(ctx: &LowerCtx<'_>, sid: crate::ssa::StructId, name: &str) -> bool {
    ctx.ast
        .objlit_method_fields
        .iter()
        .filter(|(_, methods)| methods.iter().any(|m| m == name))
        .any(|(objlit_ty, _)| matches!(ctx.aliases.get(objlit_ty), Some(Type::Obj(s)) if *s == sid))
}

fn emit_fnsig_call(
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
    let mut argv: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    let (params, ret_ty) = ctx.fn_sigs[sig_id.0 as usize].clone();
    // RFC 20260714-t262-top-clusters 刀 1 — same call-boundary
    // coercion as the direct-call terminal: an Array-literal arg
    // into an any param used to pass its typed repr raw (elem reads
    // answered undefined).
    let coerce_owned =
        crate::ssa_lower_call_terminal::coerce_args_by_param_tys(ctx, &params, args, &mut argv);
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
    for op in coerce_owned {
        ctx.emit_drop_value(op, Type::Str);
    }
    result
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
    // Accessor arity is fixed (getter 0, setter 1) — never pads.
    emit_closure_call(ctx, recv_op, offset, user_sig_id, args, true, 0)
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
    let mut site = prepare_closure_call(ctx, recv_op, offset, user_sig_id, true);
    site.argv.extend(arg_ops);
    finish_closure_call(ctx, site)
}

/// The env / fn-ptr loads and the widened ABI signature a closure call
/// needs, hoisted so both arg lanes (ExprId — which must lower its args
/// BETWEEN the loads and the call — and pre-lowered Operand) share one
/// definition of the ABI.
struct ClosureCallSite {
    fn_ptr: Operand,
    user_params: Vec<Type>,
    ret_ty: Type,
    env_first_sig: SigId,
    /// Seeded with the fixed leading slots (env, and `__this` when the
    /// callee takes a receiver); the caller appends the user args.
    argv: Vec<Operand>,
}

fn prepare_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    takes_recv: bool,
) -> ClosureCallSite {
    let closure_env = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, recv_op.clone(), offset),
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
    let fixed = 1 + usize::from(takes_recv);
    let mut abi_params = Vec::with_capacity(user_params.len() + fixed);
    abi_params.push(Type::Ptr);
    if takes_recv {
        abi_params.push(Type::Ptr);
    }
    abi_params.extend(user_params.iter().copied());
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, abi_params, ret_ty);
    let mut argv: Vec<Operand> = Vec::with_capacity(user_params.len() + fixed);
    argv.push(Operand::Value(closure_env));
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
    }
}

fn finish_closure_call(ctx: &mut LowerCtx<'_>, site: ClosureCallSite) -> Operand {
    let ClosureCallSite {
        fn_ptr,
        ret_ty,
        env_first_sig,
        argv,
        ..
    } = site;
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

fn emit_closure_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    offset: u64,
    user_sig_id: SigId,
    args: &[ExprId],
    takes_recv: bool,
    pad_n: usize,
) -> Operand {
    let mut site = prepare_closure_call(ctx, recv_op, offset, user_sig_id, takes_recv);
    let fixed = site.argv.len();
    // Same post-lower cur_block rule as emit_fnsig_call above — a
    // branching arg moves cur_block; the loads dominate the merge.
    for a in args {
        let op = ctx.lower_expr(*a);
        site.argv.push(op);
    }
    // RFC 20260714-t262-top-clusters 刀 1 — same call-boundary
    // coercion as the direct-call terminal (the leading env / `__this`
    // slots are ours; the user args align with the user param list).
    let user_params = site.user_params.clone();
    let coerce_owned = crate::ssa_lower_call_terminal::coerce_args_by_param_tys(
        ctx,
        &user_params,
        args,
        &mut site.argv[fixed..],
    );
    crate::ssa_lower_call_terminal::pad_undef_n(ctx, pad_n, &mut site.argv);
    let result = finish_closure_call(ctx, site);
    for op in coerce_owned {
        ctx.emit_drop_value(op, Type::Str);
    }
    result
}
