//! Reflection and spread over interface values: they read a struct by
//! its static layout, which a class instance behind an interface does
//! not share.

use super::sid_is_iface;
use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Whether a value of `ty` can hold an interface value whose members
/// sit at its class's offsets rather than the interface's.
fn mentions_iface(slots: &crate::iface_witness::IfaceSlots, ty: &crate::check::Type) -> bool {
    use crate::check::Type as T;
    if slots.iface_of(ty).is_some() {
        return true;
    }
    match ty {
        T::Array(x) | T::Nullable(x) | T::Promise(x) | T::MapIter(x) => mentions_iface(slots, x),
        T::Map(k, v) => mentions_iface(slots, k) || mentions_iface(slots, v),
        T::Union(xs) => xs.iter().any(|x| mentions_iface(slots, x)),
        T::Struct(fs) => fs.iter().any(|(_, x)| mentions_iface(slots, x)),
        _ => false,
    }
}

/// The reflection built-ins (`JSON.*`, `Object.*`, `Reflect.*`) and an
/// object spread read a struct by its static layout, and an interface
/// value may be a class instance laid out otherwise. The consumers that
/// read their first argument through [`lower_reflect_arg`] take the
/// dynamic lane, which reads the layout off the instance's own header;
/// every other one refuses such a value loudly rather than read the
/// wrong slots.
pub(crate) fn refuse_reflection(ctx: &LowerCtx<'_>, callee: ExprId, args: &[ExprId]) {
    use crate::ast::Expr;
    let slots = &ctx.ast.iface_slots;
    if slots.is_empty() {
        return;
    }
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*obj) else {
        return;
    };
    if !matches!(ns.as_str(), "JSON" | "Object" | "Reflect") {
        return;
    }
    let dynamic_first = matches!(
        name.as_str(),
        "stringify"
            | "keys"
            | "values"
            | "entries"
            | "getOwnPropertyNames"
            | "ownKeys"
            | "__forinKeys"
    );
    for (i, a) in args.iter().enumerate() {
        if (i > 0 || !dynamic_first)
            && ctx
                .expr_types
                .get(a)
                .is_some_and(|t| mentions_iface(slots, t))
        {
            panic!(
                "ssa-lower: `{ns}.{name}` over an interface value whose class may lay it out differently"
            );
        }
    }
}

/// A reflection built-in's subject: an interface value (or an array of
/// them) crosses into `any`, whose lane reads each object's own layout.
/// The box is a pure encoding, so the operand's stake story is the
/// caller's as before.
pub(crate) fn lower_reflect_arg(ctx: &mut LowerCtx<'_>, arg: ExprId) -> Operand {
    let op = ctx.lower_expr(arg);
    let slots = &ctx.ast.iface_slots;
    if slots.is_empty() {
        return op;
    }
    // an `as` cast moves no value: `x as any` hands over `x` itself
    let mut src = arg;
    while let crate::ast::Expr::As { expr, .. } = ctx.ast.get_expr(src) {
        src = *expr;
    }
    let Some(t) = ctx.expr_types.get(&src) else {
        return op;
    };
    if !matches!(ctx.operand_ty(&op), Type::Obj(_) | Type::Arr(_)) {
        return op;
    }
    // a binding typed `any` can still hold the struct pointer itself
    let on_iface_layout = matches!(ctx.operand_ty(&op), Type::Obj(sid) if sid_is_iface(ctx, sid));
    let whole = on_iface_layout
        || match t {
            crate::check::Type::Array(x) => slots.iface_of(x).is_some(),
            t => slots.iface_of(t).is_some(),
        };
    if whole {
        return ctx.box_to_any(op);
    }
    if mentions_iface(slots, t) {
        panic!("ssa-lower: reflecting over a value holding interface values is not supported");
    }
    op
}

/// [`refuse_reflection`] for an object spread source.
pub(crate) fn refuse_spread(ctx: &LowerCtx<'_>, src: ExprId) {
    let slots = &ctx.ast.iface_slots;
    if !slots.is_empty()
        && ctx
            .expr_types
            .get(&src)
            .is_some_and(|t| mentions_iface(slots, t))
    {
        panic!("ssa-lower: spreading an interface value whose class may lay it out differently");
    }
}
