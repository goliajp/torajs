//! `f(a, ...xs)` / `recv.name(a, ...xs)` — a call whose argument
//! list still carries a dynamic `...spread` when lowering sees it
//! (§13.3.8.1 ArgumentListEvaluation, rotation 372). The static
//! expanders ran first (parse-time `fold_static_spread`,
//! `apply_rest_args`, `apply_spread_args`); whatever reaches this
//! arm needs a RUNTIME argument count, which no fixed-argc lane can
//! express — so the full list materializes into one `Array<Any>`
//! (spread sources walk the unified iteration protocol with its
//! per-step throw check — §7.4.5 IteratorStep errors propagate
//! catchably) and the call routes to the spread kernels
//! (`__torajs_any_call_spread` / `__torajs_any_method_call_spread`),
//! which read argc off the array and enter the same dispatch tails
//! as their fixed-argc twins.
//!
//! Claimed FIRST in `ssa_lower_call::lower`'s cascade (after the
//! undeclared-callee read): every later lane assumes a compile-time
//! argument list, so a spread reaching one would mis-lower.
//! Evaluation order per §13.3.6.2: callee/receiver first, then the
//! argument list left-to-right (a spread source's iteration happens
//! at its argument position).

use torajs_rc::any_method_id;

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

/// Try to lower a spread-carrying call. Returns `None` when no
/// argument is a spread (the cascade continues).
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if !args
        .iter()
        .any(|a| matches!(ctx.ast.get_expr(*a), Expr::Spread { .. }))
    {
        return None;
    }
    if let Expr::Member { obj, name } = ctx.ast.get_expr(callee) {
        let obj = *obj;
        let name = name.clone();
        Some(lower_method_spread(ctx, obj, &name, args))
    } else {
        Some(lower_bare_spread(ctx, callee, args))
    }
}

/// Bare callee: lower it, box to Any (a fn value / closure /
/// already-any), materialize the args, enter the spread kernel.
/// `pub(crate)` for the fn-value `.apply` route
/// ([`crate::ssa_lower_call_fn_call_value`]) — a spread-carrying
/// literal argArray re-enters here with the array's elements as the
/// argument list.
pub(crate) fn lower_bare_spread(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    let raw = ctx.lower_expr(callee);
    let recv = if matches!(ctx.operand_ty(&raw), Type::Any) {
        raw.clone()
    } else {
        ctx.box_to_any(raw.clone())
    };
    let args_arr = build_args_arr(ctx, args);
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call_spread,
            vec![recv.clone(), args_arr.clone()],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(args_arr, Type::Arr(arr_any_id));
    // A Call-shaped callee (`f(1)(2, ...xs)`) is an owned Any temp
    // the runtime only borrowed (the any-call arm's account).
    ctx.release_owned_temp(callee, &raw);
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}

/// Member callee: the receiver boxes at this any-lane boundary and
/// the method dispatches by interned id + name (the
/// `ssa_lower_any_method_call` pack, minus the fixed argv).
fn lower_method_spread(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    name: &str,
    args: &[ExprId],
) -> Operand {
    let mid = any_method_id(name);
    let name_str = ctx.intern_string_literal(name);
    let raw = ctx.lower_expr(obj);
    ctx.emit_arr_mark_kind(&raw);
    let recv = if matches!(ctx.operand_ty(&raw), Type::Any) {
        raw.clone()
    } else {
        ctx.box_to_any(raw.clone())
    };
    // Ident receivers ride their variable slot along so
    // growth-relocating methods write the fresh pointer back (the
    // any-method-call arm's two shapes).
    let recv_slot = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        if let Some(info) = ctx.locals.get(n) {
            Operand::Value(info.slot)
        } else if ctx.globals.contains_key(n) {
            let gname = n.clone();
            let gref =
                ctx.f
                    .append_inst(ctx.cur_block, InstKind::GlobalRef(gname), Type::Ptr, None);
            Operand::Value(gref)
        } else {
            Operand::ConstPtrNull
        }
    } else {
        Operand::ConstPtrNull
    };
    let args_arr = build_args_arr(ctx, args);
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call_spread,
            vec![
                recv.clone(),
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                recv_slot,
                args_arr.clone(),
            ],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(args_arr, Type::Arr(arr_any_id));
    // A Call-shaped receiver is an owned Any temp the runtime only
    // borrowed (the any-method-call arm's account).
    ctx.release_owned_temp(obj, &raw);
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}

/// Materialize the full argument list into one fresh `Array<Any>`
/// (owned, rc=1 — the caller drops it after the call, releasing the
/// elements). Literal args push through `pack_any_elem` (its rc /
/// unbox ledger transfers each stake into the slot); spread sources
/// route through the unified iteration protocol: an `Array<Any>`
/// extends directly (per-slot rc_inc copy), everything else boxes
/// and materializes via `ssa_lower_arr_from_any::emit` (typed
/// arrays bridge per-slot; strings iterate per code point; custom
/// `[Symbol.iterator]` shapes and non-iterables resolve per §7.4.2
/// GetIterator at runtime). `pub(crate)` for the super-builtin
/// lane's spread flavor ([`crate::ssa_lower_call_super_builtin`]).
pub(crate) fn build_args_arr(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let literal_count = args
        .iter()
        .filter(|a| !matches!(ctx.ast.get_expr(**a), Expr::Spread { .. }))
        .count() as i64;
    let mut arr = Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_alloc_any,
            vec![Operand::ConstI64(literal_count)],
        ),
        Type::Arr(arr_any_id),
        None,
    ));
    for &aid in args {
        if let Expr::Spread { expr } = ctx.ast.get_expr(aid) {
            let inner = *expr;
            let src = ctx.lower_expr(inner);
            let mut src = src;
            let mut src_ty = ctx.operand_ty(&src);
            // String sources unfold per Unicode CODE POINT (§13.2.4.1
            // GetIterator + the §22.1.5 String iterator) — the
            // generic any-iteration would walk code units and split
            // surrogate pairs. Same pre-step as the array-literal
            // spread (`lower_spread_source`): Substr materializes to
            // owned Str, Str walks `arr_from_string` into an owned
            // per-code-point Arr<Str> the generic route then boxes.
            let mut str_arr_temp: Option<Operand> = None;
            if matches!(src_ty, Type::Substr) {
                let owned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_to_owned, vec![src]),
                    Type::Str,
                    None,
                );
                src = Operand::Value(owned);
                src_ty = Type::Str;
            }
            if matches!(src_ty, Type::Str) {
                let str_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
                let a = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_from_string, vec![src]),
                    Type::Arr(str_arr_id),
                    None,
                );
                src = Operand::Value(a);
                src_ty = Type::Arr(str_arr_id);
                str_arr_temp = Some(src.clone());
            }
            let src_is_any_arr = matches!(src_ty, Type::Arr(id)
                if matches!(ctx.arr_layouts[id.0 as usize], Type::Any));
            if src_is_any_arr {
                arr = Operand::Value(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_extend_any, vec![arr, src.clone()]),
                    Type::Arr(arr_any_id),
                    None,
                ));
                ctx.release_owned_temp(inner, &src);
                continue;
            }
            let boxed = if matches!(src_ty, Type::Any) {
                src.clone()
            } else {
                ctx.box_to_any(src.clone())
            };
            let materialized = crate::ssa_lower_arr_from_any::emit(ctx, boxed);
            // An owned source temp (a call's product) was only
            // borrowed by the iteration protocol — release it, or
            // every spread strands its stake (the array-literal
            // spread arm's account). Borrow shapes self-gate false.
            ctx.release_owned_temp(inner, &src);
            arr = Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_extend_any,
                    vec![arr, materialized.clone()],
                ),
                Type::Arr(arr_any_id),
                None,
            ));
            ctx.emit_drop_value(materialized, Type::Arr(arr_any_id));
            // The per-code-point Arr<Str> the Str pre-step minted is
            // an owned temp of ours (release_owned_temp gates on the
            // EXPR shape and cannot see it).
            if let Some(t) = str_arr_temp {
                let t_ty = ctx.operand_ty(&t);
                ctx.emit_drop_value(t, t_ty);
            }
        } else {
            let v = ctx.lower_expr(aid);
            let v_ty = ctx.operand_ty(&v);
            let (tag_op, value_op) = ctx.pack_any_elem(v, v_ty, Some(aid));
            arr = Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_push_any, vec![arr, tag_op, value_op]),
                Type::Arr(arr_any_id),
                None,
            ));
        }
    }
    arr
}
