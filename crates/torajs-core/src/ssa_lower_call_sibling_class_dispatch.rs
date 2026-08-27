//! Phase I.1 sibling-class static dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-32 of the `Expr::Call` god-arm decomp (chunks 1-31 = ... +
//! WeakRef / WeakMap / WeakSet typed-receiver method dispatch).
//!
//! For methods declared on **unrelated classes** (no inheritance
//! relation, so no shared `__dispatch_<M>`), desugar leaves the
//! `Member`-call shape intact. We resolve the receiver's static
//! class from its struct id via `aliases` (`type Foo = { ... }` →
//! `Type::Obj(sid)`) and emit the matching `__cm_<C>__<M>` static
//! call. Pure name overlap across siblings is what makes this arm
//! distinct from the parent/child dispatch path: there's no
//! shared base, so we need static-call resolution rather than
//! virtual `__dispatch_<M>`.
//!
//! Rotation 507 — a name can be BOTH: overridden inside one
//! hierarchy and declared again by an unrelated class. Desugar keeps
//! such sites Member-shape (no `__dispatch_` stub — its `__this`
//! would have to fit two unrelated bases), so this lane is where
//! the static class is finally known: when a strict descendant of it
//! declares the method, the call reads the receiver's vtable slot
//! (`ast.method_index` carries every overridden name since 507)
//! with the static class's own resolved signature — Liskov holds
//! inside the hierarchy, and the unrelated declarer's row fills the
//! same slot with its own body. The old static resolution answered
//! `Base`'s body for a `Leaf` wearing a `Base` type as soon as an
//! unrelated `name()` existed.
//!
//! **P10.6-A3 throw-propagation** — sibling-class static dispatch
//! must run the same throw-propagation gate as the regular `Call`
//! path: a may-throw method (e.g. `Generator.prototype.throw`'s
//! `Stmt::Throw(__err)` body) sets `throw_active` inside the
//! callee, and without the post-call check the throw silently
//! fails to reach the caller's try/catch (no jump emitted to the
//! handler; the rest of the calling stmt continues as if no
//! throw happened).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! `Member`-call, method name not in any class's owner table,
//! receiver isn't `Type::Obj`, no alias resolves to the matching
//! sid + class_parents entry, or `__cm_<C>__<M>` doesn't exist
//! in `fn_table`) so the caller falls through to the next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !ctx.ast.method_owners.contains_key(name) {
        return None;
    }
    let recv_id = *obj;
    let method_name = name.clone();
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    // RFC 20260705 chunk 555 park protocol — every decline below this
    // point has already lowered (or consumed a parked) receiver, so it
    // must re-park the operand for the next dispatcher; dropping it
    // would re-evaluate a side-effecting receiver downstream.
    let Type::Obj(_sid) = recv_ty else {
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    };

    // RFC 20260715-nominal-class-identity — dispatch on the receiver's
    // NAME. The layout id is shared by every same-shaped class (and by
    // a plain object literal), so a reverse lookup by shape answered
    // whichever class registered first.
    let Some(cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id) else {
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    };
    let Some(fn_name) = declaring_class_fn(ctx, &cname, &method_name) else {
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    };
    let Some(fid) = ctx.fn_table.get(&fn_name).copied() else {
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    };
    // Polymorphic below the static class → the slot; otherwise the
    // resolved body is the only one an instance can be wearing.
    let owners = ctx
        .ast
        .method_owners
        .get(&method_name)
        .cloned()
        .unwrap_or_default();
    let slot =
        ctx.ast.method_index.get(&method_name).copied().filter(|_| {
            crate::ssa_lower_call_vtable_dispatch::overridden_below(ctx, &cname, &owners)
        });

    let mut arg_ops: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    // S2.42 (rotation 240) — this lane handed every argument verbatim
    // while all its sibling call lanes route through the `arg_conv`
    // contract; an i64 into the callee's Any param arrived as raw
    // bits (two `function*` decls make `next` a sibling-owned name,
    // so `g.next(42)` came through here and the generator's
    // resumption value read back as a garbage NaN-box). The sig is
    // receiver-first: user args align to params[1..].
    let coerce_owned = match ctx.fn_sig_ids.get(&fid).copied() {
        Some(sig_id) => {
            let param_tys = ctx.fn_sigs[sig_id.0 as usize].0.clone();
            crate::ssa_lower_call_terminal::coerce_args_by_param_tys(
                ctx,
                param_tys.get(1..).unwrap_or(&[]),
                args,
                &mut arg_ops,
            )
        }
        None => Vec::new(),
    };
    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
    argv.push(recv_op);
    argv.extend(arg_ops);
    let ret_ty = ctx.f_ret_type_hint(fid);
    let cur_block = ctx.cur_block;
    let (v, may_throw) = match (slot, ctx.fn_sig_ids.get(&fid).copied()) {
        (Some(idx), Some(sig_id)) => (
            crate::ssa_lower_call_vtable_dispatch::emit_vtable_call(ctx, idx, sig_id, ret_ty, argv),
            // Any body the slot can resolve to at or below the static
            // class may be the one that throws.
            owners.iter().any(|o| {
                crate::ast::method_owner_is_in_chain(&ctx.ast.class_parents, &cname, o)
                    && ctx
                        .may_throw_fns
                        .contains(&format!("__cm_{o}__{method_name}"))
            }) || ctx.may_throw_fns.contains(&fn_name),
        ),
        _ => (
            ctx.f
                .append_inst(cur_block, InstKind::Call(fid, argv), ret_ty, None),
            ctx.may_throw_fns.contains(&fn_name),
        ),
    };
    if may_throw {
        ctx.emit_throw_check(None);
    }
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    Some(Operand::Value(v))
}

/// The `__cm_<C>__<M>` a receiver of static class `cname` would run:
/// the first class along its ancestor chain that declares `method`.
///
/// The receiver's own class is not always the answer. A subclass that
/// inherits the method declares no `__cm_` of its own, and this lane
/// only ever sees a name several unrelated classes declare — so
/// `Derived extends Base` reaches `Base`'s body while an unrelated
/// `Point` with the same method name reaches its own.
///
/// Shared with the Object.prototype arm ahead of this one, which has
/// to decline exactly the calls this lane claims.
pub(crate) fn declaring_class_fn(ctx: &LowerCtx<'_>, cname: &str, method: &str) -> Option<String> {
    let mut cur = Some(cname.to_string());
    while let Some(c) = cur {
        let fn_name = format!("__cm_{c}__{method}");
        if ctx.fn_table.contains_key(&fn_name) {
            return Some(fn_name);
        }
        cur = ctx.ast.class_parents.get(&c).cloned().flatten();
    }
    None
}
