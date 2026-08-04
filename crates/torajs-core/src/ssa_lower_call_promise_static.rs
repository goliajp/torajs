//! `Promise.<static>` cluster (`Promise.all` / `.race` / `.any` /
//! `.allSettled` / `.resolve` / `.reject`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-27 of the `Expr::Call` god-arm decomp (chunks 1-26 = ... +
//! fs_promises async wrappers).
//!
//! Three arms share the `Promise.<m>(...)` namespace:
//!
//! - **T-17.a / .b / .c / .d (v0.5.0)** — `Promise.all` / `.race` /
//!   `.any` / `.allSettled` sync fast paths. Lower `args[0]` (iterable),
//!   eval-and-drop `args[1..]` per S273 ES §27.2.4.{1,3,5,2} trailing-
//!   arg ignore, dispatch to the matching `promise_<m>_sync` intrinsic.
//!   Falls through (returns `None`) when `args.is_empty()` so a more
//!   generic call lowering can fire.
//!
//! - **P10.2-A1** — 0-arg `Promise.resolve()` / `Promise.reject()`.
//!   Spec-equivalent to `Promise.{resolve,reject}(undefined)` which
//!   shares the i64-0 sentinel ABI with `null`; synthesize
//!   `ConstI64(0)` and dispatch the non-heap fulfilled/rejected
//!   allocator (same path 1-arg primitive takes).
//!
//! - **T-15.g.1 / T-15.g.5** — 1+arg `Promise.resolve(v)` /
//!   `Promise.reject(e)`. S322 lower-and-drop trailing `args[1..]` per
//!   S272 idiom. §27.2.4.7 step 2: `Promise.resolve(p)` on a
//!   `Type::Promise` arg passes the SAME object through (identity,
//!   no mint) — reject side keeps the simple-heap path per spec
//!   (§27.2.4.6 always mints). Heap-vs-primitive dispatch by
//!   `arg_ty`; `Bool` → `coerce_bool_to_i64`; `F64` → `BitCastF64ToI64`
//!   so the value slot uniformly holds 8 bytes the receiver decodes
//!   per the promise's value class (②.6b). When `arg_ty == I64` but
//!   the field width table says this anon-ExprId slot is f64-shaped,
//!   `coerce_to_f64` + `BitCastF64ToI64` first.
//!
//! - **§27.2.4.8 (ES2024)** — `Promise.withResolvers()`. 0-arg
//!   kernel call; the runtime mints the pending promise + the two
//!   settle-function cells and answers the boxed `{promise, resolve,
//!   reject}` dynobj (`Type::Any` — every member read/call rides the
//!   any lane).
//!
//! Returns `Some(op)` on hit; `None` on miss so the caller falls
//! through to subsequent arms or the generic call lowering.

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let recv_id = *ns_id;
    let method = m_name.clone();
    let Expr::Ident(ns) = ctx.ast.get_expr(recv_id) else {
        return None;
    };
    if ns != "Promise" {
        return None;
    }
    let m = method.as_str();
    match m {
        "all" | "race" | "any" | "allSettled" if !args.is_empty() => {
            Some(lower_aggregate(ctx, eid, m, args))
        }
        "resolve" | "reject" if args.is_empty() => Some(lower_zero_arg(ctx, m)),
        "resolve" | "reject" => Some(lower_one_plus(ctx, eid, m, args)),
        "withResolvers" => Some(lower_with_resolvers(ctx, args)),
        _ => None,
    }
}

/// `Promise.withResolvers(...)` — §27.2.4.8 reads no arguments;
/// eval-and-drop them for side effects (the S273 idiom), then call
/// the 0-arg kernel. The result is the boxed `{promise, resolve,
/// reject}` dynobj (owned AnyValue).
fn lower_with_resolvers(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    for &a in args {
        let _ = ctx.lower_expr(a);
    }
    let fid = ctx.intrinsics.promise_with_resolvers;
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![]), Type::Any, None);
    Operand::Value(v)
}

/// The element form `Promise.all`'s result array has to hold, derived
/// the same way the awaiting site derives how to READ those slots — so
/// the two ends name one lane instead of two.
///
/// The checker typed this call `Promise<T[]>`, but SSA's `Type::Promise`
/// is inner-erased, so the word cannot come off the operand; it comes
/// off the checker's per-expression side table, which lowering already
/// carries (`LowerCtx::expr_types`). `recover_inner_ssa_ty` +
/// `widen_promise_inner_ty` is verbatim what `await` runs on this same
/// ExprId in `ssa_lower_member_promise_value`, width table included.
///
/// `0` = no lane could be named, which leaves the kernel on the
/// behaviour it had before this word existed.
fn all_result_elem_repr(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    let inner = crate::ssa_lower_member_promise_value::recover_inner_ssa_ty(ctx, eid);
    let Some(Type::Arr(aid)) = ctx.widen_promise_inner_ty(inner, eid) else {
        return 0;
    };
    let elem = ctx.arr_layouts[aid.0 as usize];
    let as_f64 = matches!(elem, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(&elem, as_f64, false).unwrap_or(0)
}

/// The class tag `Promise.allSettled`'s `{status, value}` records have
/// to carry so a by-name read can find their fields.
///
/// The records used to be 48 anonymous bytes with `class_tag = 0`,
/// which is invisible to every by-name lookup: an unannotated handler
/// (parameter inferred `any`) read them as `{}`. An ordinary `{x: 1}`
/// literal is any-readable because the COMPILER stamps it out of
/// `anon_stamp_pool` and emits a layout row for it, and
/// `collect_class_field_candidates` walks that pool — so the record
/// joins the same pool rather than getting a mechanism of its own.
/// The runtime cannot mint this itself: `__torajs_class_layouts` is
/// link-emitted rodata with nothing appending to it at startup.
///
/// Two tags, packed low word / high word: the fulfilled record's
/// `{status, value}` and the rejected one's `{status, reason}`. §27.2.4.2
/// names the second field differently per outcome, and one layout cannot
/// answer to two names at the same offset — so the rejected shape is
/// interned as its own struct and stamped separately. The runtime picks
/// by state; the bytes it writes are identical either way.
///
/// `0` when the element shape is not a struct here — the anonymous
/// posture, unchanged.
fn allsettled_record_tags(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    let inner = crate::ssa_lower_member_promise_value::recover_inner_ssa_ty(ctx, eid);
    let Some(Type::Arr(aid)) = inner else {
        return 0;
    };
    let Type::Obj(sid) = ctx.arr_layouts[aid.0 as usize] else {
        return 0;
    };
    let fulfilled = ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid);
    // The rejected twin: the checker's own element struct with its
    // second field renamed, so the field TYPES follow whatever it
    // settled on for T and the offsets stay identical.
    let Some(check_mod::Type::Promise(pinner)) = ctx.expr_types.get(&eid) else {
        return i64::from(fulfilled);
    };
    let check_mod::Type::Array(elem) = &**pinner else {
        return i64::from(fulfilled);
    };
    let check_mod::Type::Struct(fields) = &**elem else {
        return i64::from(fulfilled);
    };
    let [(_, status_ty), (_, value_ty)] = &fields[..] else {
        return i64::from(fulfilled);
    };
    let rejected_shape = check_mod::Type::Struct(vec![
        ("status".to_string(), status_ty.clone()),
        ("reason".to_string(), value_ty.clone()),
    ]);
    let ann = crate::check_type_to_ann::type_to_ann(&rejected_shape);
    let rejected_ty = crate::ssa_lower_parse_type::parse_type(
        Some(&ann),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    );
    let Type::Obj(rsid) = rejected_ty else {
        return i64::from(fulfilled);
    };
    let rejected = ctx.anon_stamp_pool.borrow_mut().assign_or_get(rsid);
    i64::from(fulfilled) | (i64::from(rejected) << 32)
}

/// `Promise.all/race/any/allSettled(xs, ...)` — lower `args[0]` and
/// drop the rest for side-effects per S273 / ES §27.2.4.
fn lower_aggregate(ctx: &mut LowerCtx<'_>, eid: ExprId, method: &str, args: &[ExprId]) -> Operand {
    let arr_op = ctx.lower_expr(args[0]);
    for &a in &args[1..] {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arr_op);
    // RFC 20260730 knife A — a statically non-Array argument reaches
    // the dynamic entries: §27.2.4 GetIterator on it throws at
    // runtime and the combinator answers a rejected promise instead
    // of tr rejecting the whole program at compile time. The checker
    // admits only statically non-iterable types here (Any / String
    // stay compile rejects until the tag-dispatch knife).
    if !matches!(arg_ty, Type::Arr(_)) {
        let boxed = ctx.box_to_any_from_expr(args[0], arr_op.clone());
        let fid = match method {
            "all" => ctx.intrinsics.promise_all_dyn,
            "race" => ctx.intrinsics.promise_race_dyn,
            "any" => ctx.intrinsics.promise_any_dyn,
            "allSettled" => ctx.intrinsics.promise_allsettled_dyn,
            _ => unreachable!(),
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(fid, vec![boxed]),
            Type::Promise,
            None,
        );
        // The box shares; a fresh owned temp still owes its stake
        // (the isFinite/isNaN box-and-call precedent).
        if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
            ctx.emit_drop_value(arr_op, arg_ty);
        }
        return Operand::Value(v);
    }
    let fid = match method {
        "all" => ctx.intrinsics.promise_all_sync,
        "race" => ctx.intrinsics.promise_race_sync,
        "any" => ctx.intrinsics.promise_any_sync,
        "allSettled" => ctx.intrinsics.promise_allsettled_sync,
        _ => unreachable!(),
    };
    // `all` and `allSettled` each need one word only the call site can
    // supply. race / any forward a single settled value, so the
    // awaiting site's own repr decode already covers them.
    let mut call_args = vec![arr_op.clone()];
    match method {
        "all" => call_args.push(Operand::ConstI64(all_result_elem_repr(ctx, eid))),
        "allSettled" => call_args.push(Operand::ConstI64(allsettled_record_tags(ctx, eid))),
        _ => {}
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, call_args),
        Type::Promise,
        None,
    );
    // The combinator borrows the promises array (walks slots, incs
    // what it keeps), so an Ident arg keeps its stake and its scope
    // drop — the old consume path orphaned the array AND its promises
    // (RFC 20260705 ledger #3, 35MB probe). Owned temps release here.
    ctx.release_owned_temp(args[0], &arr_op);
    Operand::Value(v)
}

/// `Promise.{resolve,reject}()` — synthesize the undefined sentinel
/// and route to the primitive fulfilled/rejected allocator.
fn lower_zero_arg(ctx: &mut LowerCtx<'_>, method: &str) -> Operand {
    let fid = match method {
        "resolve" => ctx.intrinsics.promise_alloc_fulfilled,
        "reject" => ctx.intrinsics.promise_alloc_rejected,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![Operand::ConstI64(0)]),
        Type::Promise,
        None,
    );
    // RFC 20260720-anylane-promise-methods knife 1 — the zero-arg
    // form settles with undefined; the any-lane bridge answers it
    // as such.
    let out = Operand::Value(v);
    ctx.emit_promise_stamp_repr(&out, crate::ssa_lower_promise_repr_mark::REPR_VOID);
    out
}

/// `Promise.{resolve,reject}(v, ...)` — thenable absorption + heap /
/// primitive dispatch + S322 trailing-arg side-effect.
fn lower_one_plus(ctx: &mut LowerCtx<'_>, eid: ExprId, method: &str, args: &[ExprId]) -> Operand {
    // §27.2.4.7 — `Promise.resolve(undefined)` settles with exactly
    // what `Promise.resolve()` settles with, so it takes the zero-arg
    // lane rather than boxing `undefined` as an ordinary value. Below,
    // an undefined-typed operand is not heap-shaped, so it went to the
    // primitive allocator WITHOUT the void repr stamp the zero-arg
    // form applies — and the any-lane bridge then read a tag it did
    // not recognise (`[unknown-any-tag]`, `typeof` answering
    // "object"). The arguments still lower, for effect.
    if method == "resolve"
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined | crate::check::Type::Void)
        )
    {
        for &a in args {
            let _ = ctx.lower_expr(a);
        }
        return lower_zero_arg(ctx, method);
    }
    let arg_op = ctx.lower_expr(args[0]);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);
    // §27.2.4.7 step 2 — PromiseResolve on a promise whose
    // constructor is %Promise% answers the SAME object
    // (`Promise.resolve(p) === p`; tr has no Promise subclassing, so
    // the pass-through is unconditional). Replaces the pre-spec
    // T-19.f absorption mint (`promise_resolve_thenable` — kernel
    // kept for future any-lane use). Owned-result convention: a
    // borrow-shaped arg shares (+1); an owned temp transfers as-is.
    if matches!(arg_ty, Type::Promise) && method == "resolve" {
        if !ctx.expr_transfers_ownership(args[0]) {
            ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
        }
        return arg_op;
    }
    // §27.2.4.7 step 2 through the ANY lane — the runtime kernel
    // probes the box for a %Promise% cell (pass-through, adopting
    // the caller's transferred ref) and otherwise folds the
    // fulfilled_heap + REPR_ANY stamp pair itself, so the
    // pass-through can never be stamped over. Borrow-shaped args
    // share (+1) exactly like the heap lane below.
    if matches!(arg_ty, Type::Any) && method == "resolve" {
        if !ctx.expr_transfers_ownership(args[0]) {
            ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
        }
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![arg_op]),
            Type::Promise,
            None,
        );
        return Operand::Value(v);
    }
    let is_heap = matches!(
        arg_ty,
        Type::Str
            | Type::Substr
            | Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::Any
    );
    let fid = match (method, is_heap) {
        ("resolve", false) => ctx.intrinsics.promise_alloc_fulfilled,
        ("reject", false) => ctx.intrinsics.promise_alloc_rejected,
        ("resolve", true) => ctx.intrinsics.promise_alloc_fulfilled_heap,
        ("reject", true) => ctx.intrinsics.promise_alloc_rejected_heap,
        _ => unreachable!(),
    };
    // `promise_alloc_*_heap` ADOPTS the value (caller transfers one
    // ref, pool.rs contract). A borrow-shaped arg therefore shares:
    // +1 so the source binding keeps its own stake — the old consume
    // path stole it (UAF once the promise dropped first, reuse-window
    // probe printed filler vs bun val42). Owned temps transfer their
    // fresh ref as-is.
    if is_heap && !ctx.expr_transfers_ownership(args[0]) {
        ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
    }
    // RFC 20260720-anylane-promise-methods knife 1 — the repr stamp
    // mirrors the STORED form this site emits (the I64→f64-slot
    // widening below changes it), so capture the predicates the
    // coercion chain is about to consume.
    let is_null = matches!(arg_op, Operand::ConstPtrNull);
    let stored_as_f64 = matches!(arg_ty, Type::F64)
        || (matches!(arg_ty, Type::I64)
            && ctx
                .num_f64_slots
                .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value"));
    mark_arr_value(ctx, &arg_op, &arg_ty);
    let arg_i64 = if matches!(arg_ty, Type::Bool) {
        ctx.coerce_bool_to_i64(arg_op)
    } else if matches!(arg_ty, Type::F64) {
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(arg_op),
            Type::I64,
            None,
        ))
    } else if matches!(arg_ty, Type::I64)
        && ctx
            .num_f64_slots
            .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value")
    {
        let as_f64 = ctx.coerce_to_f64(arg_op);
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(as_f64),
            Type::I64,
            None,
        ))
    } else {
        arg_op
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_i64]),
        Type::Promise,
        None,
    );
    let out = Operand::Value(v);
    if let Some(repr) =
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&arg_ty, stored_as_f64, is_null)
    {
        ctx.emit_promise_stamp_repr(&out, repr);
    }
    out
}

/// The repr stamp makes the cell any-consumable, so a typed-Arr
/// value settles as an any-readable cell too: mark its elem kind
/// (RFC 20260704 S1 self-describing posture — the bridge's boxed
/// hand-off is exactly the typed→any crossing the mark exists for).
fn mark_arr_value(ctx: &mut LowerCtx<'_>, arg_op: &Operand, arg_ty: &Type) {
    if matches!(arg_ty, Type::Arr(_)) {
        ctx.emit_arr_mark_kind(arg_op);
    }
}
