//! One plain `name: value` member of an object literal: its value
//! lowered, and the share the field owes when the value is a borrow.
//! Moved out of [`super`] whole.

use super::any_slot;
use super::declared;
use crate::ast::{Expr, ExprId, PropKey};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

#[allow(clippy::too_many_arguments)] // one member's lowering, all of it borrowed state
pub(super) fn lower_regular_field(
    ctx: &mut LowerCtx<'_>,
    name: &PropKey,
    eid: ExprId,
    declared_field_ty: Option<Type>,
    owner_ann: Option<&str>,
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
    parked: &mut Vec<usize>,
) {
    // Chunk 785 — forward the declared struct layout to a nested
    // ObjectLit field value (see `lower_field_entries`).
    if let Some(Type::Obj(inner_sid)) = declared_field_ty
        && matches!(ctx.ast.get_expr(eid), Expr::ObjectLit { .. })
    {
        let ann = owner_ann.and_then(|o| declared::field_ann(ctx, o, name));
        ctx.pin_declared_obj(inner_sid, ann.as_deref());
    }
    let v = ctx.lower_expr(eid);
    ctx.let_declared_obj_layout = None;
    let cast = match (declared_field_ty, owner_ann) {
        (Some(slot), Some(owner)) => declared::cast_member(ctx, (eid, v), slot, (owner, name)),
        _ => None,
    };
    if let Some(c) = cast {
        let ty = ctx.operand_ty(&c);
        parked.push(ctx.push_throw_temp(c, ty));
        return place(name, ty, c, field_tys, field_vals);
    }
    let v = any_slot::box_heap_value(ctx, eid, v, declared_field_ty);
    if let Some((op, ty)) = ctx.throw_temp_of(eid, &v) {
        parked.push(ctx.push_throw_temp(op, ty));
    }
    let ty = ctx.operand_ty(&v);
    // Peel value-transparent `As` wrappers (and a comma's discarded
    // left operand) before judging borrow-ness:
    // `lower_as_cast` is IDENTITY for an Any-typed source (the borrow
    // rides through the cast), a fresh IMMEDIATE box for a primitive
    // (whose inc below is a payload-gated no-op), and a bare
    // pass-through for a heap source — in every shape the inner read
    // decides whether the field owes an extra share. Pre-fix the As
    // node fell into the `_ => false` arm and the SM step struct
    // (`{value: __ys_src[i] as any, done: false}`) bare-stored a
    // borrowed box the struct drop walk then released as an owned
    // child: `function* g() { yield* [obj] }` freed the module global
    // on the first for-of drive (use-after-free that blade 1's layout
    // change surfaced as a SIGSEGV).
    let src_eid = crate::ssa_lower_return_borrow::peel_passthrough(ctx.ast, eid);
    let needs_inc = ty.is_refcounted()
        && match ctx.ast.get_expr(src_eid) {
            // An Ident read is a BORROW off its slot unless the local
            // was moved — and a name that is not a local at all (a
            // promoted module global read from a method / fn body) has
            // no move semantics, so the field share must inc too.
            // unwrap_or(false) here bare-stored the global's pointer
            // while the struct drop walk released it as an owned
            // child (`function* g() { yield obj }` — same UAF family
            // as the As shape above; a static cell's inc is
            // FLAG-gated to a no-op, so the `true` arm is safe for
            // interned singletons).
            //
            // A BORROWED binding has no stake to transfer, whatever
            // its `moved` flag says: the closure-capture preamble
            // registers its bindings `moved: true` AT BIRTH (so the
            // fn-exit drop walk skips them — the env owns the stake),
            // and reading that as "already transferred into this
            // field" bare-stored the env's reference for the struct
            // drop walk to release. `return {value: captured}` from a
            // nested fn freed the captured promise on the first call
            // (the interleave knife's then-override probe read a
            // scrubbed props slot on the second element).
            //
            // An escape-BOXED binding in its owning frame reads
            // through the box, so the load answers the box's payload
            // as a borrow while the frame exit still releases the
            // whole box — the field owes its own share. The
            // capture-box preamble registers exactly `moved: true,
            // borrowed: false`, which the two clauses above read as
            // "already transferred into this field", so the store
            // went bare and the struct drop walk released a stake
            // nobody took. Twin of the `ssa_lower_stmt_return.rs`
            // arm (rotation 326) — same fact, other consumer.
            Expr::Ident(name) => {
                ctx.locals
                    .get(name)
                    .map(|info| info.borrowed || !info.moved)
                    .unwrap_or(true)
                    || ctx.boxed_noncopy_lets.contains(name)
            }
            Expr::Member { .. } | Expr::Index { .. } => true,
            // Hoisted regex-literal singleton (fn-scope LICM) — the
            // field takes a share; see apply_borrow_rc_inc mirror.
            Expr::Regex { .. } => true,
            _ => false,
        };
    // No consume on the else side: every shape reaching it is a no-op
    // for the move-walk (Copy binding / already-moved Ident / non-Ident
    // expr) — chunk 572 removed the dead marker.
    if needs_inc {
        // Type-aware: an Any-flavored operand is a NaN-box whose raw
        // header inc the rc_inc cell gate silently skips — the share
        // must go through the payload-gated any_box_rc_inc instead
        // (immediates stay no-ops). Raw inc kept for plain heap types.
        ctx.emit_owned_result_inc(v, ty);
    }
    place(name, ty, v, field_tys, field_vals);
}

/// The member `name` set to `v` of type `ty`, a later spelling of a
/// name replacing the earlier one in place.
fn place(
    name: &PropKey,
    ty: Type,
    v: Operand,
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
) {
    if let Some(pos) = field_tys.iter().position(|(k, _)| k == name) {
        field_tys[pos] = (name.clone(), ty);
        field_vals[pos] = v;
    } else {
        field_tys.push((name.clone(), ty));
        field_vals.push(v);
    }
}
