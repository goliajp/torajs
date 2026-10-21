//! Which calls the any-method lane (`super::try_lower`) claims when
//! the receiver does not type as `any`.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

/// Does the any-method lane claim the call whose callee is `callee`:
/// a member read off a receiver that types as `any`, is nullish, or
/// has one of the admitted shapes? Answers the receiver and the name.
pub(crate) fn claims<'c>(ctx: &'c LowerCtx<'_>, callee: ExprId) -> Option<(ExprId, &'c String)> {
    // Rotation 591 — §13.3.6.2 reads the base off the REFERENCE, and
    // a type assertion does not consume one: `(o.f as any)()` calls
    // with `this === o` exactly as `o.f()` does. The shape test
    // therefore peels `As`, while every TYPE test below keeps the
    // unpeeled `callee` — the assertion is what widened the read to
    // Any, and that widening is the admission this lane wants.
    let shape = ctx.peel_as_wrappers(callee);
    let Expr::Member { obj, name } = ctx.ast.get_expr(shape) else {
        return None;
    };
    // A NULLISH receiver (`undefined.toString()`) rides this lane
    // too: the checker admitted the site (§13.2.3 GetValue is a
    // runtime TypeError) and the recv box below carries it into the
    // kernel, whose nullish arm raises the catchable TypeError.
    let recv_is_nullish = ctx
        .expr_types
        .get(obj)
        .is_some_and(|t| t.is_nullish_value());
    if !recv_is_nullish
        && !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any))
        && !non_any_recv_admitted(ctx, callee, obj, name)
    {
        return None;
    }
    Some((*obj, name))
}

/// Whether a NON-Any receiver still rides the any-method lane — the
/// eight admitted shapes, each keyed to the checker arm that admitted
/// the call as Any (so admitted and lowered shapes cannot drift).
fn non_any_recv_admitted(ctx: &LowerCtx<'_>, callee: ExprId, obj: &ExprId, name: &str) -> bool {
    // RFC 20260713-array-proto-residual blade 2 — the
    // `<any>.toString.call(x)` family: the member sugar arms
    // type the read as a concrete Function, but the read
    // lowers to a runtime any cell, so .call / .apply / .bind
    // on it ride this any lane (checker mirror in
    // route_early.rs).
    let is_fn_surface = matches!(name, "call" | "apply" | "bind")
        && matches!(
            ctx.expr_types.get(obj),
            Some(crate::check::Type::Function(..))
        );
    let sugar_fn_on_any = is_fn_surface
        && matches!(
            ctx.ast.get_expr(*obj),
            Expr::Member { obj: inner, .. }
                if matches!(ctx.expr_types.get(inner), Some(crate::check::Type::Any))
        );
    // RFC 20260725-str-method-value-reify — the same surfaces on
    // a reified String-receiver method value (a `builtin_mv`
    // binding or the inline `s.slice.call(…)` form) ride the any
    // lane too: the runtime's `call_target` re-dispatches the
    // carried mid with the thisArg as receiver (checker mirror
    // in route_early.rs).
    let builtin_mv = is_fn_surface
        && (matches!(
            ctx.ast.get_expr(*obj),
            Expr::Ident(n) if ctx.builtin_mv_locals.contains_key(n)
        ) || crate::ssa_lower_stmt_let_decl_method_value::builtin_mv_member_init_mid(
            ctx, *obj,
        )
        .is_some());
    let ns_static_fn = is_fn_surface && crate::ssa_lower_ns_static_alias::is_fn_read(ctx, *obj);
    // The same surfaces on a builtin CONSTRUCTOR value
    // (`Number.bind(null)` / `Promise.call(p, fn)`): the bare
    // namespace ident lowers to the interned ctor cell
    // (`try_builtin_ctor_ident`), whose boxed entry runs the
    // per-family ctor-as-function conversion or the catchable
    // TypeError. Checker mirror: route_early's
    // `is_builtin_ctor_read` (same proto-tag table + unbound
    // gate — the obj types Object(ns) here, not Function).
    let builtin_ctor_fn = matches!(name, "call" | "apply" | "bind")
        && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Object(_)))
        && matches!(ctx.ast.get_expr(*obj), Expr::Ident(n)
            if crate::ssa_lower_member_builtin_namespace::proto_method_tag(n).is_some());
    // Cluster #4 (test262) — a CONCRETE receiver whose member
    // read types Any: the per-family member tables' catch-all
    // answered the read (`arr.hasOwnProperty` / `fn.caller` /
    // an expando property), and the checker's general tail
    // admitted the call as Any (mirror gate — both key on the
    // callee expr's Any record). The receiver boxes at this
    // any-lane boundary below; the runtime dispatcher answers
    // by tag.
    let any_member_read = matches!(ctx.expr_types.get(&callee), Some(crate::check::Type::Any));
    // RFC 20260806 blade 3 — the checker's gate can only mark call
    // sites it types, and some routes never type their arguments
    // (`console.log(xs.slice(0))` is how this surfaced). Ask the
    // module fact directly so a patched builtin reaches the
    // dispatcher no matter which route claimed the enclosing call.
    let shadowed_builtin = !ctx.scans.proto_shadow.is_empty()
        && ctx
            .expr_types
            .get(obj)
            .and_then(crate::builtin_proto_shadow::family_of)
            .is_some_and(|f| ctx.scans.proto_shadow.shadows(f, name));
    // The namespace STATICS, one object over from the prototypes
    // above: `Object.keys = f` is a write to the constructor, and the
    // syntax-keyed routes further down the dispatch claim
    // `Object.keys(o)` without reading that object at all. Sending
    // the site here is what makes the patch reachable — the receiver
    // boxes below and the runtime dispatcher answers off the object's
    // own bag, which is where the write landed.
    let patched_ns_static = !ctx.scans.proto_shadow.is_empty()
        && matches!(ctx.ast.get_expr(*obj), Expr::Ident(n)
            if ctx.scans.proto_shadow.writes_static(n, name));
    // RFC 20260813-detached-objlit-method — the same surfaces on a
    // binding whose SLOT holds an Any while the checker named a
    // class method's signature (`const t = c.read; t.call(c)`).
    // The read minted the method value off the class-methods
    // table, so the runtime dispatcher is the only thing that can
    // re-bind it; see `any_call::callee_slot_is_any`.
    let detached_method = is_fn_surface && crate::ssa_lower_any_call::callee_slot_is_any(ctx, *obj);
    // RFC 20260820-member-call-route 刀 1 — the same surfaces on
    // an INLINE class-instance method read (`c.method.call(x)`):
    // the read types Function (the checker's class-method arm)
    // but lowers to a runtime any cell — S2.34 resolves the
    // reified class-method cell off the prototype — so only the
    // runtime dispatcher can re-bind the thisArg. Fields never
    // fire (only `__cm_`-table methods count), so fn-typed FIELD
    // reads keep the fn_call_value replay. Checker mirror:
    // route_early's `is_class_method_read`.
    let class_method_value = is_fn_surface
        && matches!(ctx.ast.get_expr(*obj), Expr::Member { obj: inner, name: m }
            if crate::ssa_lower_member::receiver_class_owns_method(ctx, *inner, m));
    // The same surfaces on an ELEMENT read whose array took the any
    // lane (`const m = [d.n]; m[0].call(d, 2)`): the checker types
    // the element by the literal (Function), the lowering boxed it
    // into Arr<Any>, so the call any-dispatches like the detached
    // binding above — the caller coerces the Any this lane answers
    // to the static replay's type.
    let any_elem_method = is_fn_surface
        && matches!(ctx.ast.get_expr(*obj), Expr::Index { obj: arr, .. }
            if crate::ssa_lower_index_any_method_call::obj_slot_elems_are_any(ctx, *arr));
    // a binding the checker types as an object whose slot holds an
    // any: an element of a class array the any world may write into
    // (`for (const p of ps)` over a `P[]` that escaped), which can
    // hold an instance of another class with methods of its own; read
    // straight off such an array (`ps[i].m()`) the same holds
    let object_in_any_slot = matches!(
        ctx.expr_types.get(obj),
        Some(crate::check::Type::Struct(_) | crate::check::Type::ClassRef(_))
    ) && (crate::ssa_lower_any_call::callee_slot_is_any(ctx, *obj)
        || matches!(ctx.ast.get_expr(*obj), Expr::Index { obj: arr, .. }
            if crate::ssa_lower_index_any_method_call::obj_slot_elems_are_any(ctx, *arr)));
    // an array or union receiver the lowering keeps in an any slot
    // (`xs: (string | number)[] | any[]`): no typed lane claims its
    // methods, so the runtime dispatcher answers them; a cast of such
    // a binding (`(xs as any[]).push(v)`) passes its value through
    let mut slot_expr = *obj;
    while let Expr::As { expr, .. } = ctx.ast.get_expr(slot_expr) {
        slot_expr = *expr;
    }
    let union_in_any_slot = matches!(
        ctx.expr_types.get(obj),
        Some(crate::check::Type::Union(_) | crate::check::Type::Array(_))
    ) && crate::ssa_lower_any_call::callee_slot_is_any(ctx, slot_expr);
    sugar_fn_on_any
        || object_in_any_slot
        || union_in_any_slot
        || builtin_mv
        || ns_static_fn
        || builtin_ctor_fn
        || any_member_read
        || shadowed_builtin
        || patched_ns_static
        || detached_method
        || class_method_value
        || any_elem_method
}
