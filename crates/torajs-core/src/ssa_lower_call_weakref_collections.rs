//! T-26 / T-26.B — WeakRef.deref + WeakMap / WeakSet method dispatch
//! pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-31 of the `Expr::Call` god-arm decomp
//! (chunks 1-30 = ... + console.<m> dispatch). Two arms share the
//! `T-26` weakref namespace so they cluster naturally:
//!
//! **Arm 1 — `wr.deref()` on a Type::WeakRef receiver.** Lowers to
//! `__torajs_weakref_deref` returning the target (rc-bumped) or null.
//! The receiver isn't consumed — the caller's drop walk handles the
//! WeakRef binding's lifetime. The Ptr-typed result is exposed as
//! `Type::Ptr` at SSA; downstream `as` casts narrow back to whatever
//! concrete heap type the user stored.
//!
//! S325 — accepts any arity for WeakRef receivers per ES §26.1.3.2
//! trailing-arg ignore. The receiver type is peeked via
//! `expr_types` so the gate stays narrow to WeakRef and doesn't
//! shadow user-class `.deref(...)` shapes. Trailing args
//! lower-and-drop after the deref call so step() side-effects fire
//! (check.rs S325 already typecheck-dropped).
//!
//! **Arm 2 — `WeakMap.set/get/has/delete` + `WeakSet.add/has/delete`.**
//! All take key (and optionally value for `WeakMap.set`) as borrows;
//! the map/set receiver stays owned by the caller's binding. The
//! runtime auto-evicts when keys die.
//!
//! Pre-flight check: only intercept when the method name is one we
//! explicitly handle for the receiver type — otherwise fall through
//! to other arms below. The receiver type is peeked without lowering
//! effects: local `Ident` is the common shape; class-field receiver
//! (`new W().m.set(k, v)` where `m: WeakMap<K,V>`) is an
//! `Expr::Member` whose checked type is `check::Type::WeakMap` —
//! mirroring the P6.1 Map / P6.2 Set fix (commit `eab88d7c`).
//! Without the Member / Index / Call branch, the receiver falls
//! through to the module.method dispatch and panics
//! `"unsupported member call shape: set"`.
//!
//! S301 — useful arity is `set:2` / others:1; lower `args[..useful]`
//! into the intrinsic Call, drop `args[useful..]` so step()-style
//! side-effect exprs fire per ES eval-then-discard semantics.
//!
//! `has` / `delete` return i64 0/1; coerce back to `Bool` so
//! downstream BinOp / console.log dispatch picks the right print
//! intrinsic.
//!
//! Returns `Some(op)` on hit; `None` on miss so the caller falls
//! through to the next arm.

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(op) = try_lower_weakref_deref(ctx, callee, args) {
        return Some(op);
    }
    try_lower_weak_collection_method(ctx, callee, args)
}

/// Arm 1 — `wr.deref()` on a `Type::WeakRef` receiver.
fn try_lower_weakref_deref(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "deref" {
        return None;
    }
    let recv_id = *obj;
    if crate::ssa_lower_recv_face::static_ty(ctx, recv_id) != Some(Type::WeakRef) {
        return None;
    }
    let recv_op = ctx.lower_expr(recv_id);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);
    let cur_block = ctx.cur_block;
    // Chunk 629 — boxed deref: the checker types the result
    // Nullable<Any>, so the SSA value is an AnyValue box (alive box
    // = target ptr with the caller's +1, cleared = ANY_UNDEF). The
    // raw-Ptr form NaN-box-misaligned every downstream Any consumer
    // (member access on a narrowed deref result was a loud reject).
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.weakref_deref_any, vec![recv_op]),
        Type::Any,
        None,
    );
    for &a in args.iter() {
        let _ = ctx.lower_expr(a);
    }
    Some(Operand::Value(v))
}

/// Arm 2 — `WeakMap.{set,get,has,delete}` / `WeakSet.{add,has,delete}`.
fn try_lower_weak_collection_method(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let m_name = name.clone();
    let weakmap_method = matches!(
        m_name.as_str(),
        "set" | "get" | "has" | "delete" | "getOrInsert" | "getOrInsertComputed"
    );
    let weakset_method = matches!(m_name.as_str(), "add" | "has" | "delete");
    if !weakmap_method && !weakset_method {
        return None;
    }

    let recv_id = *obj;
    // Every spelling, through the shared face resolver — the private
    // table this replaces did not list `Expr::As`
    // (`ssa_lower_recv_face`).
    let recv_ty_hint = crate::ssa_lower_recv_face::static_ty(ctx, recv_id);
    let do_weakmap = weakmap_method && recv_ty_hint == Some(Type::WeakMap);
    let do_weakset = weakset_method && recv_ty_hint == Some(Type::WeakSet);
    if !do_weakmap && !do_weakset {
        return None;
    }

    if args.is_empty() {
        return None;
    }
    let recv_op = ctx.lower_expr(recv_id);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);
    // 383-04 — the stage-3 upsert pair takes its own leg: the key is
    // setter-shaped (an illegal key throws, so `lower_weak_key`
    // records the pending TypeError), the second argument rides as a
    // borrowed AnyValue box (a default value, or the callbackfn the
    // kernel invokes on a miss), and the kernel answers +1-owned Any.
    if do_weakmap && matches!(m_name.as_str(), "getOrInsert" | "getOrInsertComputed") {
        let (key_op, key_raw) = lower_weak_key(ctx, args[0], true);
        let (tag, val) = if args.len() >= 2 {
            ctx.lower_to_tag_value(args[1])
        } else {
            (Operand::ConstI64(5), Operand::ConstI64(0))
        };
        let cur_block = ctx.cur_block;
        let av = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_box, vec![tag, val]),
            Type::Any,
            None,
        );
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        let target = if m_name == "getOrInsert" {
            ctx.intrinsics.weakmap_get_or_insert
        } else {
            ctx.intrinsics.weakmap_get_or_insert_computed
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(target, vec![recv_op, key_op, Operand::Value(av)]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        settle_owned_key(ctx, args[0], &key_raw);
        return Some(Operand::Value(v));
    }
    let useful = if do_weakmap && m_name == "set" { 2 } else { 1 };
    let is_setter = (do_weakmap && m_name == "set") || (do_weakset && m_name == "add");

    // RC-4 F2 — key classification per ES CanBeHeldWeakly: a
    // statically object-like key passes as its raw ptr; anything
    // else (Any / primitive / null / undefined) is encoded to
    // borrowed AnyValue bits and classified by the runtime helper
    // (set/add record a pending TypeError for an illegal key,
    // has/get/delete read it as absent). The kernels no-op on the
    // helper's NULL result.
    let (key_op, key_raw) = lower_weak_key(ctx, args[0], is_setter);
    let mut arg_ops: Vec<Operand> = vec![key_op];
    // RC-4 F2 — the WeakMap value slot is an AnyValue lane: encode
    // the value to borrowed bits (undef-aware pair extraction; the
    // kernel incs cells it keeps, primitives ride verbatim).
    if useful == 2 {
        let (tag, val) = if args.len() >= 2 {
            ctx.lower_to_tag_value(args[1])
        } else {
            // `m.set(k)` — the missing value is `undefined` per spec.
            (Operand::ConstI64(5), Operand::ConstI64(0))
        };
        let cur_block = ctx.cur_block;
        let av = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_box, vec![tag, val]),
            Type::Any,
            None,
        );
        arg_ops.push(Operand::Value(av));
    }
    for &a in args.iter().skip(useful) {
        let _ = ctx.lower_expr(a);
    }
    let (target, ret_ty) = if do_weakmap {
        match m_name.as_str() {
            "set" => (ctx.intrinsics.weakmap_set, Type::Void),
            // RC-4 F2 — get returns the AnyValue lane directly
            // (cells +1'd by the kernel, miss = undefined sentinel).
            "get" => (ctx.intrinsics.weakmap_get, Type::Any),
            "has" => (ctx.intrinsics.weakmap_has, Type::I64),
            "delete" => (ctx.intrinsics.weakmap_delete, Type::I64),
            _ => unreachable!(),
        }
    } else {
        match m_name.as_str() {
            "add" => (ctx.intrinsics.weakset_add, Type::Void),
            "has" => (ctx.intrinsics.weakset_has, Type::I64),
            "delete" => (ctx.intrinsics.weakset_delete, Type::I64),
            _ => unreachable!(),
        }
    };
    let mut full_args = Vec::with_capacity(arg_ops.len() + 1);
    full_args.push(recv_op);
    full_args.extend(arg_ops);

    let cur_block = ctx.cur_block;
    if ret_ty == Type::Void {
        ctx.f
            .append_void(cur_block, InstKind::Call(target, full_args));
        // RC-4 F2 — an illegal key on set/add recorded a pending
        // TypeError in the key helper; propagate it here.
        if is_setter {
            ctx.emit_throw_check(None);
        }
        // Chunk 634 — settle an owned-shape key temp AFTER the
        // kernel consumed it. On a setter the entry never holds a
        // stake (the key is observed weakly), so releasing the
        // temp's only ref here fires the dying broadcast and the
        // fresh entry evicts immediately — the ES-observable state
        // for a key nobody else references.
        settle_owned_key(ctx, args[0], &key_raw);
        return Some(Operand::ConstPtrNull);
    }
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(target, full_args), ret_ty, None);
    settle_owned_key(ctx, args[0], &key_raw);
    if matches!(m_name.as_str(), "has" | "delete") {
        // Emit into the CURRENT block, not the pre-settle snapshot —
        // settling an owned key temp (its drop's null-check split)
        // may have moved cur_block; the snapshot block is then
        // already terminated. `v` still dominates (defined in the
        // pre-split half).
        let b = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(b));
    }
    Some(Operand::Value(v))
}

/// Chunk 634 — release an owned-shape key temp (`new K()`, a
/// concat result, `Symbol(...)`) after the weak-collection kernel
/// call. Chunk 630 settled the Symbol / boxed lanes BEFORE the
/// kernel ran — for a setter with a legal Symbol key that freed
/// the temp and then stored a dangling pointer in the entry (and
/// registered a dying observer against freed memory). The
/// object-like fast path never settled at all — probe l12/l13:
/// 300k `wm.set(new K(i), i)` churn leaked every key (78 MB vs
/// 6.4 MB flat).
fn settle_owned_key(ctx: &mut LowerCtx<'_>, eid: ExprId, raw: &Operand) {
    if ctx.expr_owned_shape(eid) {
        ctx.release_owned_temp(eid, raw);
    }
}

/// RC-4 F2 — route a weak-collection KEY operand per ES
/// `CanBeHeldWeakly`. A statically object-like key (Obj / Arr /
/// Closure / Symbol / Promise / RegExp / Date / weak-family / Map /
/// Set) passes as its raw ptr — zero-cost fast path. Everything else
/// (Any / primitive / null / undefined; Str and BigInt cells back JS
/// *primitives* and must be rejected too) is encoded to borrowed
/// AnyValue bits and classified by the torajs-weak runtime helper:
/// `or_throw` (set/add) records a pending TypeError for an illegal
/// key, `from_any` (has/get/delete) reads it as absent. Both return
/// NULL on rejection and the kernels no-op on a NULL key.
///
/// Returns `(key_op, raw_op)` — `raw_op` is the pre-classifier
/// lowered operand the caller settles via [`settle_owned_key`]
/// AFTER the kernel call (chunk 634: the chunk-630 shape settled
/// before the kernel ran, freeing a legal Symbol key temp the
/// setter kernel then stored dangling).
fn lower_weak_key(ctx: &mut LowerCtx<'_>, eid: ExprId, is_setter: bool) -> (Operand, Operand) {
    let is_undef = matches!(ctx.expr_types.get(&eid), Some(check_mod::Type::Undefined));
    let raw = ctx.lower_expr(eid);
    let ty = ctx.operand_ty(&raw);
    if matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::Promise
            | Type::RegExp
            | Type::Date
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
    ) {
        return (raw.clone(), raw);
    }
    // A Symbol key is legal only while NOT in the `Symbol.for`
    // registry (`CanBeHeldWeakly` — a registered symbol lives
    // forever, its weak entry could never collect). The NaN-box
    // encodes a heap cell as its raw pointer bits, so the Symbol
    // ptr passes to the classifier directly — borrowed, no box
    // call, no rc traffic (ptr ↔ i64 are ABI-compatible at the
    // call site, same as arr_push_any's third param).
    if matches!(ty, Type::Symbol) {
        let helper = if is_setter {
            ctx.intrinsics.weak_key_from_any_or_throw
        } else {
            ctx.intrinsics.weak_key_from_any
        };
        let p = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(helper, vec![raw.clone()]),
            Type::Ptr,
            None,
        );
        return (Operand::Value(p), raw);
    }
    let cur_block = ctx.cur_block;
    let av = if matches!(ty, Type::Any) {
        raw.clone()
    } else {
        // Chunk 630 — the classifier only READS the key (a legal
        // key it keeps is the fast-path early return above; every
        // key reaching here is rejected-or-primitive), so the pair
        // is a +0 borrow. The old box_to_tag_value owning inc had
        // no consumer-side drop — probe-proven ~32B/call leak on
        // `wm.has("key" + i)` churn (l11, 16.1MB vs 6.4MB flat).
        let (tag, val) = if is_undef && matches!(ty, Type::Ptr) {
            (Operand::ConstI64(5), Operand::ConstI64(0))
        } else if ty.is_refcounted() {
            (Operand::ConstI64(4), raw.clone())
        } else {
            ctx.box_to_tag_value(raw.clone())
        };
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_box, vec![tag, val]),
            Type::Any,
            None,
        );
        Operand::Value(v)
    };
    let helper = if is_setter {
        ctx.intrinsics.weak_key_from_any_or_throw
    } else {
        ctx.intrinsics.weak_key_from_any
    };
    let p = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(helper, vec![av]),
        Type::Ptr,
        None,
    );
    (Operand::Value(p), raw)
}
