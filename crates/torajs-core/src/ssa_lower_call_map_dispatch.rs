//! P6.1 — `<Map>.{set|has|delete|get|clear|keys|values|entries|forEach}`
//! lowering pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-2 of the `Expr::Call` god-arm decomp (chunk-1 was
//! `ssa_lower_call_arr_ho` covering the Arr higher-order methods).
//!
//! All 9 methods share the same `Expr::Member` outer dispatch + Map
//! receiver-type detection, so they're co-located in one sibling instead
//! of split per method. Repeated shapes within Map's methods are folded
//! into 2 tiny helpers:
//! - `emit_predicate_call`: has / delete (both call an `i64`-returning
//!   intrinsic then ICmp != 0 → Bool).
//! - `emit_iter_create`: keys / values / entries (all return `MapIter`
//!   from a 1-arg `Call(intrinsic, [recv])`).
//!   `forEach` is the only big body (~190 LOC iterator loop). It is not
//!   split into a helper — that would not reduce LOC and would force
//!   threading 4 stack slots through a signature — but rotation 597
//!   moved the whole family to the sibling
//!   [`crate::ssa_lower_call_map_foreach`], which is a different move
//!   and does reduce this file.
//!
//! Returns `Some(result)` when the callee matches `<Map-typed>.{Map-method}`;
//! `None` lets the caller fall through to the Set / RegExp / Date / generic
//! method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower::call_hold::Holds;

/// Try to lower a Map-method call. Returns `Some` when dispatched.
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
        "set"
            | "get"
            | "getOrInsert"
            | "getOrInsertComputed"
            | "has"
            | "delete"
            | "clear"
            | "forEach"
            | "keys"
            | "values"
            | "entries"
    ) {
        return None;
    }
    // Receiver Map detection — every spelling, a cast included, through
    // the shared face resolver (`ssa_lower_recv_face`).
    if crate::ssa_lower_recv_face::static_ty(ctx, obj) != Some(Type::Map) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    // an owned receiver (`mkMap(i).set(1, boom())`) is live across the
    // guard, the arguments and the kernel: parked for their throw paths
    let recv_tok = ctx.park_owned_temp(obj, &recv_op);
    let runs_cb = matches!(m_name.as_str(), "forEach" | "getOrInsertComputed");
    let holds = Holds::take_if(ctx, runs_cb, obj, recv_op);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv_op);
    let out = if m_name == "get" && crate::ssa_lower_map_num_lane::is_lane_receiver(ctx, obj) {
        crate::ssa_lower_map_num_lane::emit_get(ctx, recv_op, args)
    } else {
        dispatch_map_method(ctx, m_name.as_str(), recv_op, args)
    };
    // a chained receiver (`m.set(a).set(b)`) is an owned temp; `set`
    // answers it with its own +1, so the inner ref is released here
    holds.release(ctx);
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(obj, &recv_op);
    Some(out)
}

fn dispatch_map_method(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    match method {
        "set" => emit_map_set(ctx, recv_op, args),
        "has" => {
            // S200 — spec §24.1.3.4 has(key) step 1: key defaults to undefined;
            // typed Map<K,V> with K ≠ Any never stores undefined keys, so the
            // result is fixed false. Skip the helper Call to dodge the 1-arg
            // debug_assert below.
            if args.is_empty() {
                return Operand::ConstBool(false);
            }
            emit_has(ctx, recv_op, args)
        }
        "delete" => {
            // S200 — spec §24.1.3.3 delete(key) step 1: same default-undefined
            // rule as `has`; typed Map<K,V> can't store undefined keys so the
            // no-op delete returns false.
            if args.is_empty() {
                return Operand::ConstBool(false);
            }
            emit_predicate_call(ctx, recv_op, args, ctx.intrinsics.map_delete)
        }
        "get" => emit_map_get(ctx, recv_op, args),
        "getOrInsert" => crate::ssa_lower_call_map_goi::emit_map_get_or_insert(ctx, recv_op, args),
        "getOrInsertComputed" => {
            crate::ssa_lower_call_map_goi::emit_map_get_or_insert_computed(ctx, recv_op, args)
        }
        "clear" => {
            // S264/S300 — trailing args ignored per spec §23.1.3.3 + S272
            // lower-and-drop so step()-style side-effect exprs fire per ES
            // eval-then-discard semantics.
            for &a in args.iter() {
                let _ = ctx.lower_expr(a);
            }
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_clear, vec![recv_op]),
            );
            Operand::ConstI64(0)
        }
        "keys" => emit_iter_create(ctx, recv_op, args, ctx.intrinsics.map_iter_create_keys),
        "values" => emit_iter_create(ctx, recv_op, args, ctx.intrinsics.map_iter_create_values),
        "entries" => {
            // P6.4c — `m.entries()` yields a MapIter that emits `[k, v]`
            // Array<Any> pairs per step. The runtime helper allocs the
            // 2-element array fresh each call (Map.set value held in the
            // table is rc_inc'd into the array).
            emit_iter_create(ctx, recv_op, args, ctx.intrinsics.map_iter_create_entries)
        }
        "forEach" => crate::ssa_lower_call_map_foreach::emit_map_for_each(ctx, recv_op, args),
        _ => unreachable!(),
    }
}

/// `m.set(k, v, ...trailing)` per ES §23.1.3.9. Returns the map itself
/// (S127-5) so `m.set(k1,v1).set(k2,v2)` chains; the rc_inc keeps the
/// returned value's ref independent of the caller's binding.
fn emit_map_set(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S248 — trailing slots type-checked + dropped here (widen `== 2` →
    // `>= 2`; args[2..] silent-ignored at lower-time).
    // §24.1.3.9 — an absent key or value is `undefined` (test262
    // set-methods `other.set(42)` on a Map; the arm indexed `args[1]`
    // outright and the compiler died on the one-element list,
    // rotation 651). The typed lane spells it as the pinned ANY_UNDEF
    // pair, the same word `emit_map_get` answers for a missing key.
    let key = args.first().map(|&a| lower_map_key(ctx, a));
    let str_key = matches!(key, Some(MapKey::Str(_)));
    let (k_tag, k_val, k_raw) = match key {
        Some(MapKey::Str(s)) => (Operand::ConstI64(-1), s, Some(s)),
        Some(MapKey::Pair(tag, val, raw)) => (tag, val, Some(raw)),
        None => (Operand::ConstI64(5), Operand::ConstI64(0), None),
    };
    let (v_tag, v_val, v_raw) = lower_arg_or_undefined(ctx, args.get(1).copied());
    // S312 — ES §23.1.3.9 evaluates trailing args left-to-right; pre-S312
    // skipped the lower entirely → step()-style side-effect exprs dropped
    // at lower-time. Mirror S272 idiom by lowering each trailing arg for
    // its side-effects before the Call.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    // a string key goes borrowed with no entry hint (k_tag holds -1)
    let call = match str_key {
        true => InstKind::Call(
            ctx.intrinsics.map_set_str,
            vec![recv_op, k_val, k_tag, v_tag, v_val],
        ),
        false => InstKind::Call(
            ctx.intrinsics.map_set,
            vec![recv_op, k_tag, k_val, v_tag, v_val],
        ),
    };
    ctx.f.append_void(ctx.cur_block, call);
    // Chunk 566 share — the pack's +1 is the kernel's transfer
    // stake; an owned-shape temp (`m.set("a", {n:i})` object
    // literal / concat key) still held its mint ref with no
    // consumer (~64B/iter on the mapset churn probe).
    if let (Some(&a), Some(raw)) = (args.first(), k_raw.as_ref()) {
        ctx.release_owned_temp(a, raw);
    }
    if let (Some(&a), Some(raw)) = (args.get(1), v_raw.as_ref()) {
        ctx.release_owned_temp(a, raw);
    }
    // A Map<K, number> refuses a value of another kind with a TypeError
    // (collections `value_lane`); a number never is one.
    if v_raw.is_some_and(|raw| !matches!(ctx.operand_ty(&raw), Type::I64 | Type::F64)) {
        ctx.emit_throw_check(None);
    }
    ctx.emit_rc_inc(recv_op);
    recv_op
}

/// One `(tag, value)` argument pair for the collection kernels:
/// the lowered argument, or the pinned ANY_UNDEF pair (tag 5, value 0)
/// when the call site wrote none — §24.1.3.9 / §24.2.3.1 read an
/// absent argument as `undefined`. `raw` is `None` for the absent
/// case: there is no owned temp to release.
pub(crate) fn lower_arg_or_undefined(
    ctx: &mut LowerCtx<'_>,
    arg: Option<ExprId>,
) -> (Operand, Operand, Option<Operand>) {
    match arg {
        Some(a) => {
            let (tag, val, raw, _) = ctx.lower_to_tag_value_raw(a);
            (tag, val, Some(raw))
        }
        None => (Operand::ConstI64(5), Operand::ConstI64(0), None),
    }
}

/// has / delete shared shape: 1 key arg, trailing-drop, intrinsic call
/// returning i64, ICmp != 0 → Bool.
fn emit_predicate_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
) -> Operand {
    // S264 — trailing args ignored per spec.
    debug_assert!(!args.is_empty());
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    // S296 — lower-and-drop trailing args past the 1 useful key slot
    // per ES trailing-arg ignore (S272 idiom).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_op, k_tag, k_val]),
        Type::I64,
        None,
    );
    // Chunk 566 share — settle an owned-temp key's mint ref.
    ctx.release_owned_temp(args[0], &k_raw);
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// `m.get(key)` per spec §24.1.3.6 — reads an out (tag, val) pair from
/// stack slots and rewraps as an Any-box. Empty-args fast-path returns
/// undefined directly (typed Map<K,V> can't store undefined keys → lookup
/// miss).
fn emit_map_get(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S200 — spec §24.1.3.6 step 1: key defaults to undefined; typed
    // Map<K,V> can't store undefined keys, lookup misses → return undefined
    // directly (any_box tag ANY_UNDEF = 5, val = 0).
    if args.is_empty() {
        let box_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BoxAny(5, Operand::ConstI64(0)),
            Type::Any,
            None,
        );
        return Operand::Value(box_v);
    }
    let tag_slot = ctx.alloca(Type::I64, Some("map_get_tag"));
    let val_slot = ctx.alloca(Type::I64, Some("map_get_val"));
    let (t, v) = (Operand::Value(tag_slot), Operand::Value(val_slot));
    match emit_find_index(ctx, recv_op, args) {
        Ok(idx) => ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.map_entry_get, vec![recv_op, idx, t, v]),
        ),
        Err((k_tag, k_val, k_raw)) => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_get, vec![recv_op, k_tag, k_val, t, v]),
            );
            // Chunk 566 share — settle an owned-temp key's mint ref.
            ctx.release_owned_temp(args[0], &k_raw);
        }
    }
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(tag_slot), 0),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(val_slot), 0),
        Type::I64,
        None,
    );
    // Wrap the (tag, val) into an Any-box.
    let box_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(box_v)
}

/// A lowered Map key: a `Str` slot for the borrowed-key kernels, or
/// the owned `(tag, value)` pair plus the raw operand to settle.
enum MapKey {
    Str(Operand),
    Pair(Operand, Operand, Operand),
}

/// Lower `key` exactly once. A key whose SSA type is `Str` stays
/// borrowed; everything else (a `Substr` view included) takes the
/// pair path.
fn lower_map_key(ctx: &mut LowerCtx<'_>, key: ExprId) -> MapKey {
    if !matches!(ctx.expr_types.get(&key), Some(crate::check::Type::String)) {
        let (tag, val, raw, _) = ctx.lower_to_tag_value_raw(key);
        return MapKey::Pair(tag, val, raw);
    }
    let s = ctx.lower_expr(key);
    if ctx.operand_ty(&s) == Type::Str {
        return MapKey::Str(s);
    }
    let (tag, val) = ctx.box_to_tag_value(s);
    MapKey::Pair(tag, val, s)
}

/// `m.has(k)` / `m.get(k)` key half: `Ok(index)` from
/// `__torajs_map_find_str` (-1 on a miss) for a string key, else the
/// lowered pair. Trailing arguments are evaluated and dropped; the
/// string key's owned temp is settled here.
pub(crate) fn emit_find_index(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
) -> Result<Operand, (Operand, Operand, Operand)> {
    let key = lower_map_key(ctx, args[0]);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let s = match key {
        MapKey::Str(s) => s,
        MapKey::Pair(tag, val, raw) => return Err((tag, val, raw)),
    };
    let idx = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.map_find_str, vec![recv_op, s]),
        Type::I64,
        None,
    );
    ctx.release_owned_temp(args[0], &s);
    Ok(Operand::Value(idx))
}

/// `m.has(k)`: for a string key the found index compared `>= 0`;
/// otherwise the generic `__torajs_map_has` over the pair.
fn emit_has(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    let idx = match emit_find_index(ctx, recv_op, args) {
        Ok(idx) => idx,
        Err((k_tag, k_val, k_raw)) => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_has, vec![recv_op, k_tag, k_val]),
                Type::I64,
                None,
            );
            ctx.release_owned_temp(args[0], &k_raw);
            let b = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            return Operand::Value(b);
        }
    };
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sge, idx, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}

/// keys / values / entries shared shape: trailing-drop + 1-arg Call to a
/// `map_iter_create_*` intrinsic returning MapIter.
///
/// keys / values return a stateful MapIter scanning entries[] yielding
/// the key / value side of each live (k, v) pair (P6.4b). entries yields
/// `[k, v]` Array<Any> pairs per step (P6.4c — runtime helper allocs the
/// 2-element array fresh each call, table values rc_inc'd into it).
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
