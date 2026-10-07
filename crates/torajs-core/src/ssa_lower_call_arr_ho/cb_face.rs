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

/// Is `cb` a callback with a boxed entry whose typed first parameter an
/// `src_ty` array feeds `any` elements?
fn any_elems_take_boxed_entry(ctx: &LowerCtx<'_>, cb: ExprId, fn_ty: Type, src_ty: Type) -> bool {
    let (Type::Closure(_), Type::Arr(src)) = (fn_ty, src_ty) else {
        return false;
    };
    if ctx.arr_layouts[src.0 as usize] != Type::Any
        || !matches!(
            ctx.sig_param_tys(fn_ty).and_then(|p| p.first().copied()),
            Some(Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str | Type::Obj(_))
        )
    {
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
        Expr::Ident(name) if ctx.fn_table.contains_key(name) => boxed(name),
        Expr::Ident(_) => {
            let fns = ctx.num_f64_slots.bound_closures(cb);
            !fns.is_empty() && fns.iter().all(boxed)
        }
        _ => false,
    }
}
