//! P6.2 — `<Set>.{add|has|delete|clear|keys|values|entries|forEach
//! |isSubsetOf|isSupersetOf|isDisjointFrom|union|intersection|difference
//! |symmetricDifference}` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-3
//! of the `Expr::Call` god-arm decomp (chunks 1-2 = Arr higher-order +
//! Map dispatch).
//!
//! Set storage piggy-backs on the Map runtime (value side pinned to
//! ANY_UNDEF), so add lowers to `map_set` + value pair `(ConstI64(5),
//! ConstI64(0))`, has / delete / clear forward to the matching `map_*`
//! helper unchanged; iterators reuse `map_iter_create_*` keyed on Set's
//! key side.
//!
//! Repeated shapes within Set fold into 4 helpers:
//! - `emit_key_predicate`: has / delete (1 value arg, intrinsic returns
//!   i64, ICmp != 0 → Bool).
//! - `emit_relation_predicate`: isSubsetOf / isSupersetOf / isDisjointFrom
//!   (1 other-Set arg, intrinsic returns i64, ICmp → Bool).
//! - `emit_setop`: union / intersection / difference / symmetricDifference
//!   (1 other-Set arg, intrinsic returns Set).
//! - `emit_iter_create`: keys|values / entries (1-arg `Call(intrinsic,
//!   [recv])` returning MapIter).
//!
//! `forEach` is inline (~75 LOC) with 2 micro-helpers for load_i64 / box_pair
//! mirroring the Map sibling shape; the (value, value, set) callback ABI
//! double-boxes the key side (storage's value side is the pinned ANY_UNDEF
//! so it's ignored).
//!
//! Returns `Some(result)` when the callee matches `<Set-typed>.{Set-method}`;
//! `None` lets the caller fall through to the RegExp / Date / generic
//! method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

mod setops;
use setops::{emit_relation_predicate, emit_setop};

/// Try to lower a Set-method call. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    let m_name = name;
    if !matches!(
        m_name.as_str(),
        "add"
            | "has"
            | "delete"
            | "clear"
            | "forEach"
            | "keys"
            | "values"
            | "entries"
            | "isSubsetOf"
            | "isSupersetOf"
            | "isDisjointFrom"
            | "union"
            | "intersection"
            | "difference"
            | "symmetricDifference"
    ) {
        return None;
    }
    // Receiver Set detection — mirror Map: class-field / index / call
    // receivers (Expr::Member / Index / Call) fall back to the checked type.
    let recv_ty_hint = match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => ctx.locals.get(n).map(|info| info.ty),
        Expr::Member { .. } | Expr::Index { .. } | Expr::Call { .. } | Expr::New { .. } => {
            match ctx.expr_types.get(&obj) {
                Some(check_mod::Type::Map) => Some(Type::Map),
                Some(check_mod::Type::Set) => Some(Type::Set),
                _ => None,
            }
        }
        _ => None,
    };
    if recv_ty_hint != Some(Type::Set) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv_op);
    let out = dispatch_set_method(ctx, m_name.as_str(), recv_op, args);
    // RFC 20260705 chunk 548 — a Call-shaped receiver (chained
    // `s.add(a).add(b)`) is an owned temp; `add` answers the
    // receiver with its own +1, so the inner ref is released here.
    ctx.release_owned_temp(obj, &recv_op);
    Some(out)
}

fn dispatch_set_method(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    match method {
        "add" => emit_set_add(ctx, recv_op, args),
        "has" => {
            // S200 — spec §24.2.3.7 Set.has(value) step 1: typed Set<T> with
            // T ≠ Any can't store undefined, 0-arg result is fixed false.
            if args.is_empty() {
                return Operand::ConstBool(false);
            }
            emit_key_predicate(ctx, recv_op, args, ctx.intrinsics.map_has)
        }
        "delete" => {
            // S200 — spec §24.2.3.4 same default-undefined rule as `has`.
            if args.is_empty() {
                return Operand::ConstBool(false);
            }
            emit_key_predicate(ctx, recv_op, args, ctx.intrinsics.map_delete)
        }
        "clear" => {
            // S264/S300 — trailing args ignored per spec §24.2.3.5; widen
            // `args.is_empty()` to accept-and-drop + lower-and-drop trailing
            // per S272 idiom so step()-style side-effect exprs fire.
            for &a in args.iter() {
                let _ = ctx.lower_expr(a);
            }
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_clear, vec![recv_op]),
            );
            Operand::ConstI64(0)
        }
        "isSubsetOf" => {
            emit_relation_predicate(ctx, recv_op, args, ctx.intrinsics.set_is_subset_of, 0)
        }
        "isSupersetOf" => {
            emit_relation_predicate(ctx, recv_op, args, ctx.intrinsics.set_is_superset_of, 1)
        }
        "isDisjointFrom" => {
            emit_relation_predicate(ctx, recv_op, args, ctx.intrinsics.set_is_disjoint_from, 2)
        }
        "union" => emit_setop(ctx, recv_op, args, ctx.intrinsics.set_union, 0),
        "intersection" => emit_setop(ctx, recv_op, args, ctx.intrinsics.set_intersection, 1),
        "difference" => emit_setop(ctx, recv_op, args, ctx.intrinsics.set_difference, 2),
        "symmetricDifference" => emit_setop(
            ctx,
            recv_op,
            args,
            ctx.intrinsics.set_symmetric_difference,
            3,
        ),
        "keys" | "values" => {
            // P6.4b — Set.keys / .values are aliased per spec §24.2.3.5 /
            // §24.2.3.10. Both yield the Set's elements (stored as the
            // key side of the Map).
            emit_iter_create(ctx, recv_op, args, ctx.intrinsics.map_iter_create_keys)
        }
        "entries" => {
            // P6.4c — Set.entries yields `[value, value]` per spec
            // §24.2.3.6 (same element twice — storage's "value" side is
            // ANY_UNDEF placeholder so the runtime dups the key).
            emit_iter_create(
                ctx,
                recv_op,
                args,
                ctx.intrinsics.map_iter_create_set_entries,
            )
        }
        "forEach" => emit_set_for_each(ctx, recv_op, args),
        _ => unreachable!(),
    }
}

/// `s.add(value, ...trailing)` per spec §24.2.3.1. Returns the set itself
/// (S127-5) for chained `s.add(v1).add(v2)`; the rc_inc keeps the returned
/// value's ref independent of the caller's binding. Storage value side is
/// pinned to ANY_UNDEF (tag = 5, val = 0) since Set doesn't carry value
/// payloads.
fn emit_set_add(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S248 — trailing slots type-checked + dropped here (widen `== 1` →
    // `>= 1`; args[1..] silent-ignored at lower-time).
    debug_assert!(!args.is_empty());
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    // S312 — ES §24.2.3.1 evaluates trailing args left-to-right; mirror
    // S272 idiom by lowering each trailing arg for side effects.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // ANY_UNDEF = 5 (runtime_str.c __TORAJS_ANY_UNDEF). Set value side
    // never carries data, but the Map storage helper signature still
    // takes a (tag, val) pair so we pass the pinned ANY_UNDEF.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_set,
            vec![
                recv_op,
                k_tag,
                k_val,
                Operand::ConstI64(5),
                Operand::ConstI64(0),
            ],
        ),
    );
    // Chunk 566 share — settle an owned-temp value's mint ref
    // (`s.add([1,2])`: the pack's +1 is the kernel's, the literal's
    // own stake had no consumer).
    ctx.release_owned_temp(args[0], &k_raw);
    ctx.emit_rc_inc(recv_op);
    recv_op
}

/// has / delete shared shape: 1 value arg, trailing-drop, intrinsic
/// returns i64, ICmp != 0 → Bool. Same shape as Map's
/// `emit_predicate_call` but inlined here to keep Set sibling standalone.
fn emit_key_predicate(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
) -> Operand {
    // S264 — trailing args ignored per spec.
    debug_assert!(!args.is_empty());
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    // S296 — lower-and-drop trailing args (S272 idiom).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_op, k_tag, k_val]),
        Type::I64,
        None,
    );
    // Chunk 566 share — settle an owned-temp value's mint ref.
    ctx.release_owned_temp(args[0], &k_raw);
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// keys / values / entries shared shape: trailing-drop + 1-arg
/// `Call(map_iter_create_*, [recv])` returning MapIter.
fn emit_iter_create(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
) -> Operand {
    // S277 — eval-and-drop trailing args.
    for &a in args.iter() {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_op]),
        Type::MapIter,
        None,
    );
    Operand::Value(v)
}

/// `s.forEach(cb, ...trailing)` per spec §24.2.3.6. Callback shape is
/// `(value, value, set)` — the storage key is the Set element so we
/// re-box it twice (value side is the pinned ANY_UNDEF placeholder).
/// Iterator-loop pattern mirrors Map.forEach: `map_iter_next` advances
/// a sentinel cursor through live entries, loads (k_tag, k_val) into
/// stack slots, double-re-boxes into independent Any pairs (avoids
/// double-drop on a shared box), then calls cb with devirt routing.
fn emit_set_for_each(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S316 — ES §24.2.3.6 silently ignores trailing args past cb.
    debug_assert!(!args.is_empty());
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Knife 4 mapset mirror — promoted fn-expr callback takes the
    // §24.2.3.6 thisArg as its leading boxed `__this` arg (see
    // Map.forEach above).
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name));
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            // box_to_any is a pure encoding — an owned-shape thisArg
            // temp keeps its stake in `op`; release after the loop
            // (arr_ho mirror), else iteration 2 reads freed memory.
            let boxed = ctx.box_to_any_from_expr(t, op.clone());
            this_temp = Some((t, op));
            Some(boxed)
        } else {
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            ))
        }
    } else {
        None
    };
    // S316 — trailing args lower after cb-lower so eval order is
    // cb → trailing → loop. A promoted callback's thisArg lowered
    // above.
    for &a in args.iter().skip(if promoted { 2 } else { 1 }) {
        let _ = ctx.lower_expr(a);
    }

    let i_slot = ctx.alloca(Type::I64, Some("__set_iter_i"));
    // Sentinel for runtime-side iterator (see Map.forEach above).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(-1), Operand::Value(i_slot), 0),
    );
    let kt_slot = ctx.alloca(Type::I64, Some("__set_iter_kt"));
    let kv_slot = ctx.alloca(Type::I64, Some("__set_iter_kv"));
    let vt_slot = ctx.alloca(Type::I64, Some("__set_iter_vt"));
    let vv_slot = ctx.alloca(Type::I64, Some("__set_iter_vv"));

    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    ctx.cur_block = header_blk;
    let live = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_iter_next,
            vec![
                recv_op,
                Operand::Value(i_slot),
                Operand::Value(kt_slot),
                Operand::Value(kv_slot),
                Operand::Value(vt_slot),
                Operand::Value(vv_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(live), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );

    ctx.cur_block = body_blk;
    let kt_v = load_i64(ctx, kt_slot);
    let kv_v = load_i64(ctx, kv_slot);
    // Re-box twice so each closure arg is an independently-owned Any-box
    // (avoids double-drop on a shared box).
    let v_box1 = box_pair(ctx, kt_v, kv_v);
    let v_box2 = box_pair(ctx, kt_v, kv_v);
    // Rotation 363 — argv-face callback: boxed variadic pack, not
    // the direct call (map_dispatch mirror; borrows only, so the
    // transfer inc is skipped).
    let argv_face = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
    if argv_face {
        let _ = crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
            ctx,
            &fn_val,
            fn_ty,
            vec![Operand::Value(v_box1), Operand::Value(v_box2), recv_op],
            3,
        );
    } else {
        ctx.emit_rc_inc(recv_op);
        let mut cb_args = vec![Operand::Value(v_box1), Operand::Value(v_box2), recv_op];
        if let Some(t) = &this_arg {
            cb_args.insert(0, t.clone());
        }
        let sig_skip = usize::from(this_arg.is_some());
        let _ = match known_fid {
            Some(fid) => ctx.call_fn_value_devirt(fid, fn_val.clone(), fn_ty, cb_args, sig_skip, 3),
            None => ctx.call_fn_value(fn_val, fn_ty, cb_args, sig_skip, 3),
        };
    }
    // §24.2.3.6 step 8.a.iii ReturnIfAbrupt — a throwing callback
    // ends the walk (previously the loop swallowed it).
    ctx.emit_throw_check(known_fid);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    ctx.cur_block = after_blk;
    // RFC 20260705 chunk 552 — release an inline arrow's minted env
    // after the loop consumed it.
    ctx.release_owned_temp(args[0], &fn_val);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    Operand::ConstI64(0)
}

fn load_i64(ctx: &mut LowerCtx<'_>, slot: ValueId) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    )
}

fn box_pair(ctx: &mut LowerCtx<'_>, tag: ValueId, val: ValueId) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(val)],
        ),
        Type::Any,
        None,
    )
}
