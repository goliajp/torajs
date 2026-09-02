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
//! - **`Promise.resolve` / `Promise.reject`** — the zero-arg,
//!   pass-through, any-lane and heap/primitive allocator arms plus
//!   the §27.2.4 static-slot patch consult live in
//!   [`crate::ssa_lower_call_promise_resolve`] (rotation 448 split).
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
        "allKeyed" | "allSettledKeyed" if !args.is_empty() => Some(
            crate::ssa_lower_call_promise_keyed::lower_keyed(ctx, m, args),
        ),
        "resolve" | "reject" => Some(crate::ssa_lower_call_promise_resolve::lower_resolve_reject(
            ctx, eid, m, args,
        )),
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
/// A `T | null` element names the ANY lane instead. A raw slot cannot
/// carry the difference between "null" and "the value": a Str-formed
/// array reads a NULL slot back as undefined, and a `number | null`
/// one reads it as `0`, both silently. Nor can the static word settle
/// it — `Type::Ptr` folds null and undefined together, so mapping it
/// to either one would turn the other into it.
///
/// Naming ANY hands the question to the runtime, which never had to
/// guess: the boxed lane boxes each element off THAT promise's own
/// stamp, and `Promise.resolve(null)` stamps REPR_NULL while
/// `Promise.resolve(undefined)` stamps REPR_VOID. The conflation only
/// exists in the collapsed static type; per element the two were
/// always distinct. The checker types a bare `null` element
/// `Nullable(String)` (`check::promise_static`), so the all-null
/// array — which used to leave the result UNSTAMPED and throw at
/// attach — arrives here as the same shape.
///
/// An element form with no raw-slot shape at all takes the same road,
/// for the same reason rather than a weaker one: "no raw lane" is
/// precisely the condition the tagged lane exists to serve. An
/// all-`undefined` input reached here as `Type::Ptr`, answered `None`,
/// and left the result UNSTAMPED — which the any-param handler then
/// refused, loudly, on an array it could have described perfectly
/// well. `0` survives only where the result is not an array at all.
fn all_result_elem_repr(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    if elem_is_nullable(ctx, eid) {
        return crate::ssa_lower_promise_repr_mark::REPR_ANY;
    }
    let inner = crate::ssa_lower_member_promise_value::recover_inner_ssa_ty(ctx, eid);
    let Some(Type::Arr(aid)) = ctx.widen_promise_inner_ty(inner, eid) else {
        return 0;
    };
    let elem = ctx.arr_layouts[aid.0 as usize];
    let as_f64 = matches!(elem, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(&elem, as_f64, false)
        .unwrap_or(crate::ssa_lower_promise_repr_mark::REPR_ANY)
}

/// Whether the checker settled this `Promise.all`'s result element on
/// a nullable type. Read off the same per-expression side table
/// `allsettled_record_tags` uses — the SSA element word is where the
/// nullability is lost, so it has to be asked before that.
fn elem_is_nullable(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    let Some(check_mod::Type::Promise(inner)) = ctx.expr_types.get(&eid) else {
        return false;
    };
    let check_mod::Type::Array(elem) = &**inner else {
        return false;
    };
    matches!(**elem, check_mod::Type::Nullable(_))
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
        ("status".into(), status_ty.clone()),
        ("reason".into(), value_ty.clone()),
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

/// The form each `allSettled` record's value slot has to hold — the
/// same problem `all`'s result array has, in the one other place that
/// buries a value where the awaiting site's own repr decode cannot
/// reach it. An element settled through the any lane carries a NaN box,
/// and the record's field is typed T.
///
/// Read off the record struct's SECOND field, so it follows whatever
/// the checker settled on rather than being re-derived.
fn allsettled_value_repr(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    let inner = crate::ssa_lower_member_promise_value::recover_inner_ssa_ty(ctx, eid);
    let Some(Type::Arr(aid)) = inner else {
        return 0;
    };
    let Type::Obj(sid) = ctx.arr_layouts[aid.0 as usize] else {
        return 0;
    };
    let Some(value_ty) = ctx.struct_layouts[sid.0 as usize].get(1).map(|(_, t)| *t) else {
        return 0;
    };
    let as_f64 = matches!(value_ty, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(&value_ty, as_f64, false).unwrap_or(0)
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
    //
    // Rotation 449 — a typed Array whose ELEMENT type is a plain
    // value (not Promise, not Any) rides the same dynamic road: the
    // sync kernels' typed walk reads every slot as a promise
    // pointer, so a raw scalar slot dereferences as a cell (silent
    // forever-pending unpatched; a real SIGSEGV under a patched
    // `resolve`, whose consult re-boxes each slot as a heap cell),
    // where §27.2.4.1.3 step 6.i wants each element wrapped by
    // promiseResolve. The `as any` spelling is the only road such an
    // array has here — the checker rejects it bare. Boxing marks the
    // element kind, so the any-lane iteration decodes slots right.
    let sync_elem_form = match arg_ty {
        Type::Arr(aid) => matches!(ctx.arr_layouts[aid.0 as usize], Type::Promise | Type::Any),
        _ => false,
    };
    if !sync_elem_form {
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
        "allSettled" => {
            call_args.push(Operand::ConstI64(allsettled_record_tags(ctx, eid)));
            call_args.push(Operand::ConstI64(allsettled_value_repr(ctx, eid)));
        }
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
