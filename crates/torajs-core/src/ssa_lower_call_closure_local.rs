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
    try_lower_with_this(ctx, eid, callee, args, None)
}

/// Rotation 261 — the `.call`/`.apply` face entry: `this_arg` is the
/// EXPLICIT thisArg expression (§20.2.3.3 / §20.2.3.1), boxed into
/// the promoted `__this` argv slot ahead of the user args. The plain
/// entry above passes `None` and a promoted binding's direct call
/// seeds `undefined` exactly as before.
pub(crate) fn try_lower_with_this(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
    this_arg: Option<ExprId>,
) -> Option<Operand> {
    let Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let info = resolve_closure_binding(ctx, callee_name)?;
    let Type::Closure(user_sig_id) = info.ty else {
        return None;
    };

    // RFC 20260708-variadic — a `(...args: E[]) => R`-typed binding
    // never static-dispatches: one binding serves closures of any
    // declared arity (with or without the real-argc slot), so the
    // call boxes its args and routes through the boxed dual entry —
    // which has no `__this` slot, so an explicit-this replay stays
    // loud instead of dropping the receiver.
    if ctx.variadic_locals.contains(callee_name) || global_argv_face_binding(ctx, callee_name) {
        if this_arg.is_some() {
            return None;
        }
        return Some(lower_variadic_call(ctx, &info, user_sig_id, args));
    }

    let env_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    // RFC 20260722-find-miss chunk C — calling a find/findLast miss
    // binding must be a catchable TypeError (bun: not a function),
    // not a jump through bytes past the sentinel header. No-op for
    // plain bindings.
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(
        ctx,
        callee,
        &Operand::Value(env_ptr),
    );
    let fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
        Type::Ptr,
        None,
    );
    // Prepend Type::Ptr to the user-facing param list to get the
    // underlying signature with the env first.
    // RFC 20260717-fnexpr-this-channel knife 2W cut 2 — a promoted
    // mixed-profile fn-expr binding (face reads + direct calls) has
    // the `__this: any` param after `__env` in its native ABI; the
    // binding's Type::Closure sig sheds it (ssa_lower_closure hidden
    // skip), so a direct call re-inserts the slot with a boxed
    // `undefined` (strict-mode call-site `this`, §10.2.1.2).
    let needs_this = ctx.ast.fnexpr_recv_locals.contains(callee_name);

    let (user_params, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
    let mut env_first_params = Vec::with_capacity(user_params.len() + 3);
    env_first_params.push(Type::Ptr);
    // RFC 20260810-indirect-argc-abi S1 — the hidden I64
    // `__torajs_argc` sits at ABI position 1 of every `__env`-first
    // entry (JSC callframe-header shape).
    env_first_params.push(Type::I64);
    if needs_this {
        env_first_params.push(Type::Any);
    }
    env_first_params.extend(user_params.iter().copied());
    let env_first_sig = intern_fn_sig(ctx.fn_sigs, env_first_params, ret_ty);

    // P0.5 mirror — Type::Any param boxes the concrete arg.
    // S126-3 see direct fn-call P0.9 in ssa_lower.
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 4);
    argv.push(Operand::Value(env_ptr));
    // S1 argc slot — the REAL user argument count (beyond-arity args
    // included; they're evaluated but never enter argv).
    argv.push(Operand::ConstI64(args.len() as i64));
    let mut owned_temps: Vec<(ExprId, Operand)> = Vec::new();
    let mut coerce_owned: Vec<(Operand, Type)> = Vec::new();
    if needs_this {
        match this_arg {
            // `.call`/`.apply` face — the explicit thisArg boxes
            // into the `__this` slot; the callee's param is a +0
            // borrow, so a fresh-owned thisArg releases after the
            // call like every other arg (chunk-569 SHARE contract).
            Some(t) => {
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
            // `undefined` NaN-box into the `__this` slot.
            None => {
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
        }
    }
    // RFC 20260810-indirect-argc-abi S3.4 — the length-only tier's
    // second argc push (into an injected injected argc user
    // slot) retired with the injection: every env-first reader now
    // rides the S1 hidden argc pushed above. Args beyond the declared
    // list still lower (ES §13.3.6.1 ArgumentListEvaluation side
    // effects) but don't enter argv.
    for (i, a) in args.iter().enumerate() {
        // Chunk 641 — empty `[]` arg allocs with the param's layout.
        let raw = ctx
            .try_lower_empty_array_arg(*a, user_params.get(i))
            .unwrap_or_else(|| ctx.lower_expr(*a));
        // Chunk 569 — snapshot pre-box owned temps for the
        // post-call release; borrow-shape args pass +0 and keep
        // their stake (no inc: the body never drops params, no
        // consume: TS args share).
        owned_temps.push((*a, raw));
        if i >= user_params.len() {
            continue; // beyond-arity: evaluated, value unused
        }
        // RFC 20260707 chunk 626 — typed array into an Arr<Any>
        // param marks the block's elem kind (T-11 call-boundary
        // widen; self-gating no-op for other type pairs).
        if let Some(p) = user_params.get(i) {
            ctx.mark_arr_arg_for_any_param(*p, &raw);
        }
        // This lane used to spell the conversions out itself, and it
        // only ever carried the Any directions — so an I64 arg into a
        // param some other call site widened to F64 went in raw:
        // `const f = (b: number) => …; f(6.5); f(2)` printed 6.5
        // twice, the callee reading integer bits out of an f64 slot.
        let converted = match user_params.get(i) {
            Some(&p) => {
                crate::ssa_lower_call_arg_conv::emit_arg_conv(ctx, p, *a, raw, &mut coerce_owned)
            }
            None => raw,
        };
        argv.push(converted);
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
        for (op, ty) in coerce_owned {
            ctx.emit_drop_value(op, ty);
        }
        return Some(Operand::ConstPtrNull);
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
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    Some(Operand::Value(v))
}

/// RFC 20260708-variadic — the boxed-dual-entry call lane for a
/// `(...args: E[]) => R`-typed binding. Args box into the shared
/// `pack_any_argv` ledger; the runtime `closure_call_variadic`
/// invokes the callee's `__boxed_<name>` adapter with the REAL argc
/// (a missing adapter answers a catchable TypeError). The Any
/// result coerces to the sig's fixed-prefix return type; heap-typed
/// returns transfer the box's +1 owned cell out via pointer unbox.
/// Resolve the callee Ident to its Closure-typed slot info — a
/// local, or (RFC 20260709-closure-global chunk 1) a module-level
/// closure binding (`const add = (a, b) => a + b` read from a
/// named-fn body) materialized from the global ref; the rest of the
/// lane (env load / argc / arg boxing / CallIndirect) is
/// shape-identical for both.
/// Rotation 365 — a TOP-LEVEL argv-face binding resolves through the
/// globals fallback, not `ctx.locals`, so the let-decl lane that
/// fills `variadic_locals` never saw it (a promoted binding has no
/// LetDecl lowering in the calling fn). `closure_argv_locals` is the
/// argv chain's own binding-name record and names exactly the
/// bindings whose lifted body grew the argc/argv slots — a raw
/// declared-sig dispatch on one of those reads its argv param off an
/// arg value (the knife-7 SIGSEGV shape). The locals miss is what
/// scopes the name key: a fn-local binding of the same name shadows
/// the global and keeps its own route.
fn global_argv_face_binding(ctx: &LowerCtx<'_>, callee_name: &str) -> bool {
    !ctx.locals.contains_key(callee_name) && ctx.ast.closure_argv_locals.contains(callee_name)
}

pub(crate) fn resolve_closure_binding(
    ctx: &mut LowerCtx<'_>,
    callee_name: &str,
) -> Option<crate::ssa_lower::LocalInfo> {
    if let Some(i) = ctx.locals.get(callee_name).copied() {
        return Some(i);
    }
    let Some(&Type::Closure(sig)) = ctx.globals.get(callee_name) else {
        return None;
    };
    let g = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(callee_name.to_string()),
        Type::Ptr,
        None,
    );
    Some(crate::ssa_lower::LocalInfo {
        slot: g,
        ty: Type::Closure(sig),
        moved: false,
        borrowed: true,
        scope_depth: 0,
    })
}

fn lower_variadic_call(
    ctx: &mut LowerCtx<'_>,
    info: &crate::ssa_lower::LocalInfo,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    let env_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    emit_variadic_boxed_call(ctx, Operand::Value(env_ptr), user_sig_id, args)
}

/// Shared boxed-dual-entry emit for a variadic-annotated callable
/// slot — the local-binding arm above and the struct-field
/// member-call arm ([`crate::ssa_lower_call_struct_method_dispatch`])
/// both land here once they've loaded the closure env.
pub(crate) fn emit_variadic_boxed_call(
    ctx: &mut LowerCtx<'_>,
    env_ptr: Operand,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
) -> Operand {
    let (argv, boxed_slots) = crate::ssa_lower_any_method_call::pack_any_argv(ctx, args);
    emit_variadic_call_conv(
        ctx,
        env_ptr,
        argv,
        args.len() as i64,
        boxed_slots,
        user_sig_id,
    )
}

/// The call + return-conversion half of the boxed variadic dispatch,
/// shared by the ExprId-packing entry above and the value-packing
/// HOF-loop entry ([`crate::ssa_lower_call_arr_ho_loop`]'s argv-face
/// downgrade, rotation 363): `closure_call_variadic(env, argv, argc)`
/// → drop the slots the packer boxed → throw check → unbox per the
/// callee's declared return type.
pub(crate) fn emit_variadic_call_conv(
    ctx: &mut LowerCtx<'_>,
    env_ptr: Operand,
    argv: crate::ssa::ValueId,
    argc: i64,
    boxed_slots: Vec<Option<Operand>>,
    user_sig_id: crate::ssa::SigId,
) -> Operand {
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.closure_call_variadic,
            vec![env_ptr, Operand::Value(argv), Operand::ConstI64(argc)],
        ),
        Type::Any,
        None,
    );
    for slot in boxed_slots.into_iter().flatten() {
        ctx.emit_drop_value(slot, Type::Any);
    }
    ctx.emit_throw_check(None);
    let (_, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize];
    match ret_ty {
        // A discarded void result and a true Any consumer both take
        // the box as-is (undefined box for void bodies).
        Type::Any | Type::Void => Operand::Value(result),
        Type::F64 => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![Operand::Value(result)]),
            Type::F64,
            None,
        )),
        // Width-narrowed number returns truncate toward zero — the
        // same documented width-vs-any boundary as the boxed-entry
        // param unbox.
        Type::I64 | Type::I32 => {
            let d = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![Operand::Value(result)]),
                Type::F64,
                None,
            );
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::FpToSi(Operand::Value(d)),
                Type::I64,
                None,
            ))
        }
        Type::Bool => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_bool, vec![Operand::Value(result)]),
            Type::Bool,
            None,
        )),
        // Heap-typed returns (Str / Arr / Obj / …): the adapter
        // boxed the body's +1 owned cell (tag 4); unboxing the
        // pointer bits transfers that reference — the NaN-box is a
        // value, not a separate heap object, so there is nothing
        // else to release.
        other if other.is_refcounted() => {
            let raw = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_unbox_value, vec![Operand::Value(result)]),
                Type::I64,
                None,
            );
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::IntToPtr(Operand::Value(raw)),
                other,
                None,
            ))
        }
        other => panic!(
            "not yet supported: ssa-lower: variadic call return type {other:?} \
             (RFC 20260708-variadic chunk 3)"
        ),
    }
}
