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
//! `forEach` is the only big body (~170 LOC iterator loop) and stays
//! inline — splitting it to a helper would not reduce LOC and would
//! force threading 4 stack slots through a signature.
//!
//! Returns `Some(result)` when the callee matches `<Map-typed>.{Map-method}`;
//! `None` lets the caller fall through to the Set / RegExp / Date / generic
//! method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

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
    // Receiver Map detection — every spelling, through the shared
    // face resolver. This used to be a private table of AST shapes
    // that did not list `Expr::As`, so a cast receiver reached the
    // cascade's terminal panic ("unsupported member call shape:
    // set"); see `ssa_lower_recv_face`.
    if crate::ssa_lower_recv_face::static_ty(ctx, obj) != Some(Type::Map) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    // Rotation 550 — an owned receiver (`mkMap(i).set(1, boom())`) is
    // live across the guard, every argument's lower and the kernel;
    // park it for their throw paths (387MB / 600k caught throws
    // before).
    let recv_tok = ctx.park_owned_temp(obj, &recv_op);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv_op);
    let out = dispatch_map_method(ctx, m_name.as_str(), recv_op, args);
    // RFC 20260705 chunk 548 — a Call-shaped receiver (chained
    // `m.set(a).set(b)`) is an owned temp; `set` answers the
    // receiver with its own +1, so the inner ref is released here.
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
            emit_predicate_call(ctx, recv_op, args, ctx.intrinsics.map_has)
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
        "forEach" => emit_map_for_each(ctx, recv_op, args),
        _ => unreachable!(),
    }
}

/// `m.set(k, v, ...trailing)` per ES §23.1.3.9. Returns the map itself
/// (S127-5) so `m.set(k1,v1).set(k2,v2)` chains; the rc_inc keeps the
/// returned value's ref independent of the caller's binding.
fn emit_map_set(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S248 — trailing slots type-checked + dropped here (widen `== 2` →
    // `>= 2`; args[2..] silent-ignored at lower-time).
    debug_assert!(args.len() >= 2);
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    let (v_tag, v_val, v_raw, _) = ctx.lower_to_tag_value_raw(args[1]);
    // S312 — ES §23.1.3.9 evaluates trailing args left-to-right; pre-S312
    // skipped the lower entirely → step()-style side-effect exprs dropped
    // at lower-time. Mirror S272 idiom by lowering each trailing arg for
    // its side-effects before the Call.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_set,
            vec![recv_op, k_tag, k_val, v_tag, v_val],
        ),
    );
    // Chunk 566 share — the pack's +1 is the kernel's transfer
    // stake; an owned-shape temp (`m.set("a", {n:i})` object
    // literal / concat key) still held its mint ref with no
    // consumer (~64B/iter on the mapset churn probe).
    ctx.release_owned_temp(args[0], &k_raw);
    ctx.release_owned_temp(args[1], &v_raw);
    ctx.emit_rc_inc(recv_op);
    recv_op
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
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        return Operand::Value(box_v);
    }
    // S264 — trailing args ignored per spec.
    debug_assert!(!args.is_empty());
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    // S296 — lower-and-drop trailing args past the 1 useful key slot per
    // ES §24.1.3.6 trailing-arg ignore (S272 idiom).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // Out-slots for (tag, value).
    let tag_slot = ctx.alloca(Type::I64, Some("map_get_tag"));
    let val_slot = ctx.alloca(Type::I64, Some("map_get_val"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_get,
            vec![
                recv_op,
                k_tag,
                k_val,
                Operand::Value(tag_slot),
                Operand::Value(val_slot),
            ],
        ),
    );
    // Chunk 566 share — settle an owned-temp key's mint ref.
    ctx.release_owned_temp(args[0], &k_raw);
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

/// `m.forEach(cb, ...trailing)` per spec §23.1.3.5. Iterator-loop pattern:
/// `map_iter_next` advances a sentinel cursor (-1 = start) through live
/// entries, loads (key_tag, key_val, val_tag, val_val) into stack slots,
/// re-boxes into Any pairs, then calls `cb(value, key, map)`. Devirts to
/// direct call when args[0] is a known Closure/FnDecl.
fn emit_map_for_each(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    // S316 — ES §23.1.3.5 silently ignores trailing args past cb. check.rs
    // S270 typecheck-drops them; mirror lower-and-drop here (S272 idiom).
    debug_assert!(!args.is_empty());
    // Lower the callback (Closure or FnSig). Devirt opportunity if args[0]
    // is an Expr::Closure or Ident → known FuncId.
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Knife 4 mapset mirror (arr_ho shape) — a promoted fn-expr
    // callback takes the §23.1.3.5 thisArg as its leading boxed
    // `__this` arg (undefined box when absent); the call aligns sig
    // params past it via sig_skip.
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

    let i_slot = ctx.alloca(Type::I64, Some("__map_iter_i"));
    // Sentinel: cursor == -1 (i64) tells runtime to start from entries[0]
    // (insertion-order walk); each call advances cursor to the next live
    // entry's index.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(-1), Operand::Value(i_slot), 0),
    );
    let kt_slot = ctx.alloca(Type::I64, Some("__map_iter_kt"));
    let kv_slot = ctx.alloca(Type::I64, Some("__map_iter_kv"));
    let vt_slot = ctx.alloca(Type::I64, Some("__map_iter_vt"));
    let vv_slot = ctx.alloca(Type::I64, Some("__map_iter_vv"));

    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));

    // header — advance the iterator; exit when no more live entries.
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

    // body — re-box the (tag, payload) pairs into Any-boxes, then call
    // the closure with (value, key, map) per spec §23.1.3.6.
    ctx.cur_block = body_blk;
    let kt_v = load_i64(ctx, kt_slot);
    let kv_v = load_i64(ctx, kv_slot);
    let vt_v = load_i64(ctx, vt_slot);
    let vv_v = load_i64(ctx, vv_slot);
    let k_box = box_pair(ctx, kt_v, kv_v);
    let v_box = box_pair(ctx, vt_v, vv_v);
    // Rotation 363 — an argv-face callback (collector's HOF-anon
    // arm admits `forEach` on ANY receiver spelling, this channel
    // included) must not take the direct / devirt call: its
    // reshaped sig leads with the synthetic argv pointer, and the
    // positional (value, key, map) would land in it (probe x1:
    // silent no-op loop). Route through the boxed variadic pack —
    // borrows only, the adapter's materialize incs what it stores,
    // so the direct lane's per-iteration transfer inc is skipped.
    let argv_face = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
    if argv_face {
        let _ = crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
            ctx,
            &fn_val,
            fn_ty,
            vec![Operand::Value(v_box), Operand::Value(k_box), recv_op],
            3,
        );
    } else {
        // The Map receiver is passed as the 3rd callback arg per
        // spec. rc_inc since each iteration transfers a fresh ref
        // into the closure.
        ctx.emit_rc_inc(recv_op);
        let mut cb_args = vec![Operand::Value(v_box), Operand::Value(k_box), recv_op];
        if let Some(t) = &this_arg {
            cb_args.insert(0, t.clone());
        }
        let sig_skip = usize::from(this_arg.is_some());
        let _ = match known_fid {
            Some(fid) => ctx.call_fn_value_devirt(fid, fn_val.clone(), fn_ty, cb_args, sig_skip, 3),
            None => ctx.call_fn_value(fn_val, fn_ty, cb_args, sig_skip, 3),
        };
    }
    // §24.1.3.9 / §24.2.3.6 step 8.a.iii ReturnIfAbrupt — a throwing
    // callback ends the walk (previously the loop swallowed it).
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
