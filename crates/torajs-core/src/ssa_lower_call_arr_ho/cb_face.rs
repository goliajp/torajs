//! The face an array iteration lane calls its callback through.
//!
//! A typed array whose alias escaped into the `any` world, and an
//! `any[]`, hand the callback `any` elements. A callback whose first
//! parameter is a typed slot would bind such an element converted on
//! the direct call; through its boxed entry the adapter's tag test runs
//! the typed body on a fitting value and hands any other to the
//! all-`any` twin (§10.2.1.3 binds an argument as it is), which the
//! check pipeline mints for these callbacks
//! (`check_monomorph_any_widen::struct_escape::escaped_cb`).

use crate::ast::{Expr, ExprId};
use crate::num_width::SlotKey;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// Does a lane that calls the callback `cb` once per element of an
/// `src_ty` array take the argv face? A callback that reads `arguments`
/// does (rotation 363); so does one whose typed first parameter is fed
/// `any` elements, when it has a boxed entry.
pub(crate) fn cb_takes_argv_face(
    ctx: &LowerCtx<'_>,
    cb: ExprId,
    fn_ty: Type,
    src_ty: Type,
) -> bool {
    crate::ast::argv_face_callback::lane_takes_argv_face(ctx.ast, cb)
        || any_elems_take_boxed_entry(ctx, cb, fn_ty, src_ty)
}

/// The callback type and argv-face flag of the `method` lane at call
/// `eid`. When `any` elements send the callback through its boxed
/// entry, its answer is what the twin may give back in any kind
/// (`d.map(v => v + 1)` on a "5" element answers "51"), and the
/// declared return would convert it. A `map` / `flatMap` /
/// `Array.from` whose product the width analysis lowers as `Arr<Any>`
/// (`num_width::hof_receivers`) and a `reduce` accumulator of a scalar
/// kind keep the answer as the box it comes back in: the callback is
/// called through a face returning `any`.
pub(crate) fn cb_face(
    ctx: &mut LowerCtx<'_>,
    cb: ExprId,
    fn_ty: Type,
    src_ty: Type,
    method: &str,
    eid: ExprId,
) -> (Type, bool) {
    if !any_elems_take_boxed_entry(ctx, cb, fn_ty, src_ty)
        && !any_seed_takes_boxed_entry(ctx, cb, fn_ty, method, eid)
    {
        return (fn_ty, cb_takes_argv_face(ctx, cb, fn_ty, src_ty));
    }
    let Type::Closure(sig) = fn_ty else {
        unreachable!("a boxed-entry callback is a lifted closure");
    };
    let (params, ret) = ctx.fn_sigs[sig.0 as usize].clone();
    let scalar = matches!(
        ret,
        Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str
    );
    let keep = match method {
        "map" | "flatMap" | "from" => {
            ret != Type::Void && ctx.num_f64_slots.slot_escapes_any(&SlotKey::Anon(eid.0))
        }
        "reduce" | "reduceRight" => scalar,
        _ => false,
    };
    if !keep || ret == Type::Any {
        return (fn_ty, true);
    }
    let any_ret = crate::ssa_lower_interners::intern_fn_sig(ctx.fn_sigs, params, Type::Any);
    (Type::Closure(any_ret), true)
}

/// [`cb_face`] for `Array.from(src, cb)`. Only an array source can hand
/// a typed callback slot any elements; a Set / Map / iterator source
/// materializes as `Arr<Any>` without being one, and keeps the face a
/// callback reading `arguments` takes.
pub(crate) fn from_face(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    fn_ty: Type,
    src_ty: Type,
    eid: ExprId,
) -> (Type, bool) {
    if matches!(
        ctx.expr_types.get(&args[0]),
        Some(crate::check::Type::Array(_))
    ) {
        return cb_face(ctx, args[1], fn_ty, src_ty, "from", eid);
    }
    (
        fn_ty,
        crate::ast::argv_face_callback::lane_takes_argv_face(ctx.ast, args[1]),
    )
}

/// Is `cb` a callback with a boxed entry whose typed first parameter an
/// `src_ty` array feeds `any` elements?
fn any_elems_take_boxed_entry(ctx: &LowerCtx<'_>, cb: ExprId, fn_ty: Type, src_ty: Type) -> bool {
    let (Type::Closure(_), Type::Arr(src)) = (fn_ty, src_ty) else {
        return false;
    };
    ctx.arr_layouts[src.0 as usize] == Type::Any && typed_head_boxed(ctx, cb, fn_ty)
}

/// Is `cb` a reducer seeded with an `any` value at call `eid`? The
/// first step binds the seed to its typed accumulator parameter as it
/// is (§23.1.3.24 step 8.b.iii), whatever the elements are.
fn any_seed_takes_boxed_entry(
    ctx: &LowerCtx<'_>,
    cb: ExprId,
    fn_ty: Type,
    method: &str,
    eid: ExprId,
) -> bool {
    matches!(method, "reduce" | "reduceRight")
        && matches!(fn_ty, Type::Closure(_))
        && matches!(ctx.ast.get_expr(eid), Expr::Call { args, .. }
            if args.get(1).is_some_and(|s| ctx.expr_types.get(s) == Some(&crate::check::Type::Any)))
        && typed_head_boxed(ctx, cb, fn_ty)
}

/// Does `cb` take a typed first parameter and have a boxed entry?
fn typed_head_boxed(ctx: &LowerCtx<'_>, cb: ExprId, fn_ty: Type) -> bool {
    if !matches!(
        ctx.sig_param_tys(fn_ty).and_then(|p| p.first().copied()),
        Some(Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str | Type::Obj(_))
    ) {
        return false;
    }
    let boxed = |n: &String| {
        ctx.fn_table
            .get(n)
            .is_some_and(|f| ctx.boxed_entries.contains_key(f))
    };
    // a binding names every lifted closure it may hold
    match ctx.ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => boxed(fn_name),
        Expr::Ident(name) if !ctx.locals.contains_key(name) && ctx.fn_table.contains_key(name) => {
            boxed(name)
        }
        Expr::Ident(_) => {
            let fns = ctx.num_f64_slots.bound_closures(cb);
            !fns.is_empty() && fns.iter().all(boxed)
        }
        _ => false,
    }
}

/// A promoted callback is either the inline Closure itself or a
/// knife-2 variable-routed binding (`fnexpr_recv_locals`) — both
/// native ABIs carry the leading `__this`; missing the Ident shape
/// ran the loop with shifted args (rotation 260). Only the methods
/// whose loop threads a receiver count: map / filter / forEach take
/// the §23.1.3 thisArg, and reduce / reduceRight (RFC 20260905
/// residue 1) take undefined — §23.1.3.24 calls the reducer with no
/// receiver, but a promoted reducer still owns the leading slot, and
/// leaving it out fed the accumulator into it.
pub(super) fn promoted_callback(ctx: &LowerCtx<'_>, cb: ExprId, method: &str) -> bool {
    let promoted = matches!(ctx.ast.get_expr(cb),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || ctx.ast.is_fnexpr_recv_use(cb);
    promoted
        && matches!(
            method,
            "map" | "filter" | "forEach" | "reduce" | "reduceRight"
        )
}
