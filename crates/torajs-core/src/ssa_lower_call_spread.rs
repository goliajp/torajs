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

mod this_first;
mod unpack;

use this_first::{lower_this_first_argv_spread, rest_ctor_head};

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
    if let Some(op) = try_lower_headless_argv_spread(ctx, callee, args) {
        return op;
    }
    let raw = ctx.lower_expr(callee);
    let recv = if matches!(ctx.operand_ty(&raw), Type::Any) {
        raw
    } else {
        ctx.box_to_any(raw)
    };
    let args_arr = build_args_arr(ctx, args);
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_call_spread, vec![recv, args_arr]),
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

/// RFC 20260816-headless-argv-face — the blade that face's own doc
/// registered as owed: "the runtime-width packing is the follow-up
/// this face's next blade owes". Until it landed,
/// `retain_off_spread_sites` dropped any head-less candidate with a
/// spread call site, and the bare fn name then died loud at the
/// callee boxing below — `box_to_any element type FnSig`.
///
/// A head-less body with a runtime argv channel wants exactly two
/// things at the call: the count, and a pointer to that many
/// contiguous NaN-boxes. Both fall out of the array this lane was
/// building anyway — an `Array<Any>`'s slots ARE contiguous
/// NaN-boxes (`torajs_arr::layout`), so `len` comes off the cell and
/// the slot base off the data pointer. No boxed adapter, no
/// forwarder, and no static width anywhere on the path.
///
/// The head offset is not read: this array was allocated three
/// instructions ago and only pushed into, so logical[0] is slot 0
/// (a deque head only moves under `shift` / `unshift`).
///
/// Restricted to callees with no declared user parameter — exactly
/// what the arguments-object family writes (`function f() { …
/// arguments[i] … }`) and what a `constructor() { … arguments … }`
/// parent takes from `super(...xs)`. A declared-parameter callee
/// never reaches here: `wrap_dynamic_spread_callees` turned it into
/// its relay closure, whose boxed adapter unboxes the declared slots
/// per type and enters the body with the same argc / argv pair
/// (`ssa_lower_boxed_entry::relay`).
fn try_lower_headless_argv_spread(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    // a generic callee's site names its all-`any` clone
    let name = ctx.call_retargets.get(&callee).unwrap_or(name).clone();
    // The this-first lane (rotation 616 / 627) wants the same two
    // values, a head wider: `super(...xs)` desugars to
    // `__cm_P__ctor(__this, __new_target, ...xs)`, a twin's
    // `C.m.apply(o, xs)` to `__smany_C__m(o, ...xs)`, so the head
    // arguments lower as themselves and only the tail becomes argv.
    if let Some(&head) = ctx.ast.this_first_argv_fns.get(&name) {
        return lower_this_first_argv_spread(ctx, &name, args, head, true, true);
    }
    // A class factory carrying its constructor's channel
    // (`arguments_object_ctor_factory`) is the same shape with no
    // head: no relay is minted for a synthetic callee, so the
    // declared parameters unpack here.
    if name.starts_with("__new_") && ctx.ast.headless_argc_fns.contains(&name) {
        let argv = ctx.ast.headless_argv_fns.contains(&name);
        return lower_this_first_argv_spread(ctx, &name, args, 0, true, argv);
    }
    // A rest constructor or its factory, with or without a count
    // channel: `apply_rest_args` takes a spread only past the fixed
    // parameters, and a synthetic callee has no relay, so
    // `new C(...xs)` / `super(...xs)` on `constructor(a, ...r)` died
    // at the callee boxing below.
    if let Some(head) = rest_ctor_head(ctx, &name) {
        let argc = if head == 0 {
            ctx.ast.headless_argc_fns.contains(&name)
        } else {
            ctx.ast.this_first_argc_fns.contains_key(&name)
        };
        return lower_this_first_argv_spread(ctx, &name, args, head, argc, false);
    }
    if !ctx.ast.headless_argv_fns.contains(&name) || !ctx.ast.headless_argc_fns.contains(&name) {
        return None;
    }
    let declared_user = ctx.ast.stmts.iter().find_map(|s| match s {
        crate::ast::Stmt::FnDecl {
            name: n, params, ..
        } if *n == name => Some(params.iter().filter(|p| !p.name.starts_with("__")).count()),
        _ => None,
    })?;
    if declared_user != 0 {
        return None;
    }
    let target = *ctx.fn_table.get(&name)?;
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let args_arr = build_args_arr(ctx, args);
    // The list's own iteration may have thrown (§7.4.5 — a generator
    // source, an iterator getter); that throw is pending BEFORE the
    // call, and the post-call check keyed on the target may be
    // elided for a target that cannot throw. Check here, releasing
    // the list on the way out.
    ctx.emit_throw_check_owned(None, args_arr, Type::Arr(arr_any_id));
    let argc = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, args_arr, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let argv = ctx.emit_arr_data_ptr(args_arr);
    let ret_ty = ctx.f_ret_type_hint(target);
    let call_args = vec![Operand::Value(argc), argv];
    let result = if ret_ty == Type::Void {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(target, call_args));
        Operand::ConstPtrNull
    } else {
        Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, call_args),
            ret_ty,
            None,
        ))
    };
    ctx.emit_drop_value(args_arr, Type::Arr(arr_any_id));
    ctx.emit_throw_check(Some(target));
    Some(result)
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
        raw
    } else {
        ctx.box_to_any(raw)
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
                recv,
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                recv_slot,
                args_arr,
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
/// elements). Literal args push through `pack_any_elem`, which gives
/// the slot a stake of its own; a fresh arg's own stake is released
/// after the push. Spread sources
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
            // Same §13.2.4.1 GetIterator step as the array-literal
            // spread: a source holding the cell is `undefined`, and
            // the typed routes below read its backing store.
            crate::ssa_lower_nullable_guard::emit_undef_cell_guard(ctx, inner, &src);
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
                (src, src_ty) = ctx.emit_arr_from_string(src, -1);
                str_arr_temp = Some(src);
            }
            // The direct extend reads the backing store; §13.2.4.1
            // reads the iterator. A module that installs a
            // `Symbol.iterator` stands this shortcut down — see the
            // array-literal twin in `ssa_lower_array_spread`.
            let src_is_any_arr = matches!(src_ty, Type::Arr(id)
                if matches!(ctx.arr_layouts[id.0 as usize], Type::Any))
                && !ctx.scans.proto_shadow.iterator_hook_written();
            if src_is_any_arr {
                arr = Operand::Value(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_extend_any, vec![arr, src]),
                    Type::Arr(arr_any_id),
                    None,
                ));
                ctx.release_owned_temp(inner, &src);
                continue;
            }
            let boxed = if matches!(src_ty, Type::Any) {
                src
            } else {
                ctx.box_to_any(src)
            };
            let materialized = crate::ssa_lower_arr_from_any::emit(ctx, boxed);
            // An owned source temp (a call's product) was only
            // borrowed by the iteration protocol — release it, or
            // every spread strands its stake (the array-literal
            // spread arm's account). Borrow shapes self-gate false.
            ctx.release_owned_temp(inner, &src);
            arr = Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_extend_any, vec![arr, materialized]),
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
            // the slot took a stake of its own, so a fresh argument's
            // original one goes (the array literal's account)
            ctx.release_owned_temp(aid, &v);
        }
    }
    arr
}
