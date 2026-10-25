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
//! `forEach` lives in [`foreach`] with its 2 micro-helpers for load_i64 /
//! box_pair, mirroring the Map sibling shape; the (value, value, set)
//! callback ABI double-boxes the key side (storage's value side is the
//! pinned ANY_UNDEF so it's ignored).
//!
//! Returns `Some(result)` when the callee matches `<Set-typed>.{Set-method}`;
//! `None` lets the caller fall through to the RegExp / Date / generic
//! method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower::call_hold::Holds;

mod foreach;
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
    // Receiver Set detection — mirror Map, through the shared face
    // resolver (`ssa_lower_recv_face`).
    if crate::ssa_lower_recv_face::static_ty(ctx, obj) != Some(Type::Set) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    // Rotation 550 — an owned receiver is live across the guard, the
    // arguments' lowers and the kernel; park it for their throw paths.
    let recv_tok = ctx.park_owned_temp(obj, &recv_op);
    let holds = Holds::take_if(ctx, m_name == "forEach", obj, recv_op);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv_op);
    let out = dispatch_set_method(ctx, m_name.as_str(), recv_op, args);
    // RFC 20260705 chunk 548 — a Call-shaped receiver (chained
    // `s.add(a).add(b)`) is an owned temp; `add` answers the
    // receiver with its own +1, so the inner ref is released here.
    holds.release(ctx);
    ctx.unpark_owned_temp(recv_tok);
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
        "forEach" => foreach::emit_set_for_each(ctx, recv_op, args),
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
    // §24.2.3.1 — `s.add()` adds `undefined` (rotation 651; the arm
    // indexed `args[0]` outright). Shared spelling with `Map.set`.
    let (k_tag, k_val, k_raw) =
        crate::ssa_lower_call_map_dispatch::lower_arg_or_undefined(ctx, args.first().copied());
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
    if let (Some(&a), Some(raw)) = (args.first(), k_raw.as_ref()) {
        ctx.release_owned_temp(a, raw);
    }
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
