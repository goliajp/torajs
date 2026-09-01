//! 551-03 — the any-widen retarget of a call through a closure
//! binding: `const tag = (strs: string[], …) => …; tag`…``. The
//! checker admitted the Any template object against the typed param
//! by cloning the LIFTED body with that slot widened
//! (`check_monomorph_any_widen`, alias route in
//! `check_type_of_call/any_widen.rs`) and recorded the clone under
//! this call's ExprId. The clone is never constructed: the call takes
//! the ORIGINAL cell's env and enters the clone directly — the same
//! two lanes the parent arm drives through the cell, with the target
//! known:
//!
//! - a variadic binding (rest param / argv face) rides the boxed dual
//!   entry: `__boxed_<clone>(env, argv, argc)` in place of the runtime
//!   `closure_call_variadic` dispatch that reads the entry off the
//!   cell — argv padded to the adapter's fixed width, which the
//!   runtime dispatcher otherwise supplies;
//! - a plain binding takes the env-first native ABI: a direct `Call`
//!   of the clone in place of the `CallIndirect` through the cell's
//!   fn_ptr, args converted against the CLONE's param types (the
//!   widened slot is `any` there).
//!
//! Recv-first, argv-face, async and self-named bodies never reach
//! here — the admit gate excludes them. A retarget that does not name
//! a lifted clone belongs to the generic lane and is left alone.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

use super::bindings::{
    global_argv_face_binding, global_fnsig_mismatch_binding, global_variadic_value_binding,
};
use super::{coerce_any_result, resolve_closure_binding};

pub(super) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let mono = ctx.call_retargets.get(&eid)?.clone();
    if !ctx.ast.widen_clone_origin.contains_key(&mono) {
        return None;
    }
    let Expr::Ident(name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let name = name.clone();
    // The checker recorded this retarget against a closure binding;
    // anything else here is a pipeline disagreement, and falling back
    // to the cell would hand the typed body the Any it was widened
    // away from — loud instead.
    let info = resolve_closure_binding(ctx, &name).unwrap_or_else(|| {
        panic!("ssa-lower: retargeted closure binding `{name}` did not resolve")
    });
    let Type::Closure(user_sig_id) = info.ty else {
        panic!("ssa-lower: retargeted binding `{name}` is not closure-typed")
    };
    let target = *ctx.fn_table.get(&mono).unwrap_or_else(|| {
        panic!("ssa-lower: widened closure clone `{mono}` missing from fn_table")
    });
    let env_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(
        ctx,
        callee,
        &Operand::Value(env_ptr),
    );
    // The route follows the CLONE's shape first: a rest-taking body
    // dispatches through the boxed entry wherever the call stands (a
    // capture of the binding in another body has no entry in that
    // body's variadic table).
    let variadic = clone_takes_rest(ctx, &mono)
        || ctx.variadic_locals.contains(&name)
        || global_argv_face_binding(ctx, &name)
        || global_variadic_value_binding(ctx, &name)
        || global_fnsig_mismatch_binding(ctx, &name);
    Some(if variadic {
        let (_, ret_ty) = ctx.fn_sigs[user_sig_id.0 as usize].clone();
        boxed_entry_call(ctx, Operand::Value(env_ptr), target, &mono, args, ret_ty)
    } else {
        native_call(ctx, eid, Operand::Value(env_ptr), target, args)
    })
}

/// Does the clone declare a rest param?
fn clone_takes_rest(ctx: &LowerCtx<'_>, mono: &str) -> bool {
    ctx.ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name, params, .. }
            if name == mono && params.iter().any(|p| p.is_rest))
    })
}

/// The boxed dual entry of the clone, called directly: `(env, argv,
/// argc) -> Any`, argv at the adapter's fixed width, the Any result
/// coerced to the binding's declared return like the runtime
/// dispatch's answer is.
fn boxed_entry_call(
    ctx: &mut LowerCtx<'_>,
    env: Operand,
    target: FuncId,
    mono: &str,
    args: &[ExprId],
    ret_ty: Type,
) -> Operand {
    let (bfid, _) = *ctx
        .boxed_entries
        .get(&target)
        .unwrap_or_else(|| panic!("ssa-lower: widened closure clone `{mono}` has no boxed entry"));
    let packed = crate::ssa_lower_any_method_call::pack_any_argv_padded(
        ctx,
        args,
        crate::ssa_lower_boxed_entry::MAX_BOXED_PARAMS,
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            bfid,
            vec![
                env,
                Operand::Value(packed.argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    packed.release(ctx);
    ctx.emit_throw_check(None);
    coerce_any_result(ctx, result, ret_ty)
}

/// The clone through its env-first native ABI — `__env`, the hidden
/// argc, then the user params — the parent arm's static shape with
/// `Call` for `CallIndirect`. Args convert against the clone's own
/// param types; beyond-arity args evaluate and drop (§13.3.6.1).
fn native_call(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    env: Operand,
    target: FuncId,
    args: &[ExprId],
) -> Operand {
    let sid = ctx.fn_sig_ids[&target];
    let (abi, ret_ty) = ctx.fn_sigs[sid.0 as usize].clone();
    debug_assert!(
        matches!(abi.first(), Some(Type::Ptr)) && matches!(abi.get(1), Some(Type::I64)),
        "widened closure clone must carry the env-first head"
    );
    let user_params = &abi[2..];
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 2);
    argv.push(env);
    argv.push(Operand::ConstI64(args.len() as i64));
    let mut temps = crate::ssa_lower_call_arg_temps::ArgTemps::new();
    for (i, a) in args.iter().enumerate() {
        let raw = ctx
            .try_lower_empty_array_arg(*a, user_params.get(i))
            .unwrap_or_else(|| ctx.lower_expr(*a));
        temps.push(ctx, *a, raw);
        let Some(&p) = user_params.get(i) else {
            continue;
        };
        ctx.mark_arr_arg_for_any_param(p, &raw);
        argv.push(crate::ssa_lower_call_arg_conv::emit_arg_conv(
            ctx,
            p,
            *a,
            raw,
            &mut temps.coerce,
        ));
    }
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut argv);
    let result = if ret_ty == Type::Void {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(target, argv));
        None
    } else {
        Some(Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, argv),
            ret_ty,
            None,
        )))
    };
    temps.check_and_release(ctx);
    result.unwrap_or(Operand::ConstPtrNull)
}
