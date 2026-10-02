//! A borrow-shaped return value takes its own stake.
//!
//! The caller owns whatever a call returns and drops it. A return
//! expression that is a READ of something still owned elsewhere — a
//! join over bindings, an array element, a field — answers that
//! owner's reference, so handing it out bare lets the caller's drop
//! take the owner's stake (a freed cell under a live object), and
//! "fixing" that by marking the owner moved strands the owner instead.
//! Each shape below retains the answer and leaves every binding its
//! scope drop. Split out of `ssa_lower_stmt_return::lower_return_value`
//! (verbatim move; the join arm now answers here instead of falling
//! through to a consume walk that marks nothing under a join).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `Some(answer)` when `src_eid` (the `As`-peeled return expression)
/// is one of the borrow shapes; the answer already carries its stake
/// and the caller skips the consume walk. `None` for everything else.
pub(crate) fn retain_borrowed_answer(
    ctx: &mut LowerCtx,
    src_eid: ExprId,
    v: &Operand,
) -> Option<Operand> {
    // A read its lowering already answered OWNED carries its stake and
    // aliases nothing: no retain, and no consume walk either. A keyed
    // read through an `any` key is one (`const k: any = …; return
    // xs[k]`): the element arm below retained it a second time (one
    // box per call, 11.4 MB vs 1.7 MB flat), and without that arm the
    // walk marked `xs` moved and stranded the whole array (46.7 MB).
    // An Ident-target assignment is another (`mint_consumer_stake`
    // takes the +1 and records the eid): the walk marked the target
    // moved on top of that stake, so the slot's own share never
    // released (`return (a = s + "!")` stranded one string per call,
    // 7.5 MB vs 1.5 MB flat; over a shared source the surplus lands
    // on a long-lived cell and RSS cannot see it), and it reached the
    // rhs too — `return (a = xs[0])` over a LOCAL array marked `xs`
    // moved and stranded the whole array per call (32.3 MB vs 1.7 MB).
    // Joins are judged below — their owned form is per-site.
    let is_read = matches!(
        ctx.ast.get_expr(src_eid),
        Expr::Index { .. }
            | Expr::OptIndex { .. }
            | Expr::Member { .. }
            | Expr::OptChain { .. }
            | Expr::Assign { .. }
    );
    if is_read && ctx.expr_owned_shape(src_eid) {
        return Some(*v);
    }
    // A join (`?:`, `??`, `&&`, `||`) over borrows answers a borrow of
    // whichever arm ran, and the consume walk marks none of the arms'
    // locals moved (it cannot know which one leaves). The caller owns
    // every return value, so the answer takes its own stake here and
    // both arms keep their scope drops. A join its lowering already
    // unified to owned (`expr_transfers_ownership`) carries that stake.
    let is_join = matches!(
        ctx.ast.get_expr(src_eid),
        Expr::Ternary { .. } | Expr::Nullish { .. }
    ) || matches!(ctx.ast.get_expr(src_eid), Expr::BinOp { op, .. }
        if matches!(op, crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr));
    if is_join {
        // The consume walk marks nothing under a join, so answering
        // here is what falling through to it did.
        if !ctx.expr_transfers_ownership(src_eid) {
            let ty = ctx.operand_ty(v);
            ctx.emit_owned_result_inc(*v, ty);
        }
        return Some(*v);
    }
    // RFC 20260708-closure-argv-face (generalized chunk 674) —
    // `return a[i]` where `a` is a local Arr<Any> hands back a
    // box BORROWING the array's elem stake (get_any_boxed is a
    // borrow read). The historical blanket fix marked `a` moved
    // — skipping its scope drop and stranding one array per
    // call (probe ag8: named fn `return a[0]` leaked; the
    // materialized `__torajs_arguments` was the same shape).
    // Retain the payload instead: the box leaves self-owned,
    // the array keeps its scope drop, and nothing takes a moved
    // mark (the index is read, never handed out). Gated on the
    // receiver's static Arr<Any> type — other Any-producing
    // index lanes may answer owned boxes where a retain would
    // leak.
    if let Expr::Index { obj, .. } = ctx.ast.get_expr(src_eid)
        && let Expr::Ident(obj_name) = ctx.ast.get_expr(*obj)
        && ctx.operand_ty(v) == Type::Any
        && ctx.locals.get(obj_name).is_some_and(|info| {
            matches!(info.ty, Type::Arr(id)
                if ctx.arr_layouts[id.0 as usize] == Type::Any)
        })
    {
        let retained = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_retain, vec![*v]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(retained));
    }
    // Rotation 326 — the typed sibling of the retain above:
    // `return xs[i]` on a typed Arr<T> with refcounted T answers
    // the slot's value as a BORROW (load_dyn reads the element
    // in place; the slot keeps its stake), while the fn-return
    // contract hands the caller an owned value. Without the +1
    // the synthesized boxed entry boxed the borrow into `any`
    // (owned by NaN-box contract) and the caller's drop stole
    // the slot's stake (census: `() => bs[0]` on a `bigint[]`
    // global underflowed the live element). Local and global
    // receivers alike; Str/Substr receivers never reach here
    // (s[i] answers a fresh owned Substr), and the Any-elem
    // shape took the anyv_retain arm above.
    if let Expr::Index { obj, .. } = ctx.ast.get_expr(src_eid) {
        let v_ty = ctx.operand_ty(v);
        let elem_matches = matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Array(_)))
            && v_ty.is_refcounted()
            && !matches!(v_ty, Type::Any | Type::Substr);
        if elem_matches {
            ctx.emit_rc_inc(*v);
            return Some(*v);
        }
    }
    // A member read that is not already owned (`return this.s`,
    // `return p.name`, `return a.b.c`) answers the FIELD'S OWN stake —
    // a bare `Load`. Every caller owns what a call returns and drops
    // it, so that drop took the field's stake away from under the
    // object: `{ const r = nameOf(p); }` freed `p.name`'s cell and
    // the next same-size allocation read back through it (`p.name`
    // answered a later string's bytes; bun answers the original). The
    // refcount wrapped below zero instead of double-freeing, which is
    // why churn stayed flat and Guard Malloc stayed quiet. For an
    // OWNED local receiver the consume walk "fixed" this by marking
    // the local moved — its cell and every other field stranded per
    // call (20.8 MB vs 1.6 MB flat). The answer takes its own stake;
    // the receiver, whatever it is, keeps its drop.
    if matches!(
        ctx.ast.get_expr(src_eid),
        Expr::Member { .. } | Expr::OptChain { .. }
    ) && !ctx.expr_owned_shape(src_eid)
    {
        let ty = ctx.operand_ty(v);
        if ty.is_refcounted() {
            ctx.emit_owned_result_inc(*v, ty);
            return Some(*v);
        }
    }
    None
}
