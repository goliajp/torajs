//! The share an assignment's target owes when [`super`]'s rhs reads
//! a borrow. Split out at the 500-line limit; a child module reaches
//! the parent's private items directly, so nothing changed visibility.

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(super) fn apply_borrow_rc_inc(ctx: &mut LowerCtx<'_>, v: &Operand, value: ExprId) {
    let v_is_refcounted = ctx.operand_ty(v).is_refcounted();
    if !v_is_refcounted {
        return;
    }
    // Peel value-transparent `As` wrappers before judging the rhs
    // shape — `lower_as_cast` answers the inner operand untouched for
    // a heap source and for an already-Any one, so the inner read is
    // what decides whether the target owes a share. Unpeeled, the
    // `As` node fell to `_ => false` ("a fresh value that transfers")
    // and the target stored the source binding's pointer bare:
    // `dst = src as any; return dst` handed the caller a cell the
    // source's scope drop then released, and 400 allocations later the
    // read came back as the churn string. The uncast `dst = src` is
    // correct, which is exactly what makes this shape easy to miss.
    let mut value = value;
    while let Expr::As { expr, .. } | Expr::Sequence { right: expr, .. } = ctx.ast.get_expr(value) {
        value = *expr;
    }
    let needs_inc = match ctx.ast.get_expr(value) {
        // String indexing emits a fresh owned Substr view, not an
        // element borrow — inc'ing it here left the extra reference
        // undropped (chunk 561, mirror of the let-decl alias fix).
        // Chunk 717 — a literal-key any-member read (`o["k"]`)
        // answers owned the same way; its eid is recorded in
        // `owned_member_reads`.
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
                && !ctx.owned_member_reads.contains(&value)
        }
        // Chunk 637 — a Member read whose owned-receiver lowering
        // detached the result already carries this consumer's stake;
        // inc'ing again would strand the original (mirror of the
        // let-decl alias re-check).
        Expr::Member { .. } => !ctx.owned_member_reads.contains(&value),
        // Reading a named binding for an assignment is always a SHARE
        // (TS has no move semantics): the source keeps its stake and
        // stays readable, the target takes +1. This holds for alias
        // and previously-consumed bindings too — their cell is alive
        // (the canonical owner holds it), so the inc mints the
        // target's own stake (chunk 564; replaced the consume-then-
        // skip-inc transfer that let the target's drop-old steal the
        // source's stake — cross-scope UAF, asm-proven). Non-binding
        // Idents (lifted closures, fn names) mint owned values and
        // keep transferring. Global-slot reads borrow the same way
        // (chunk 558).
        Expr::Ident(src) => ctx.locals.contains_key(src) || ctx.globals.contains_key(src),
        // Hoisted regex-literal singleton (fn-scope LICM,
        // `ssa_lower_lit::lower_regex`) — the slot takes a share of
        // the fn-owned compile; without the +1 the slot's drop-old
        // stole the fn's stake (UAF on the next occurrence).
        Expr::Regex { .. } => true,
        // A Ternary / Nullish / `&&`-`||` join over pure borrows, and
        // a Member / Index-target assignment, answer a borrow; an
        // owned-unified join or an Ident-target assignment recorded
        // itself in owned_member_reads and transfers its fresh stake.
        // The target is a consumer like any other: `y = cond ? ys :
        // xs; y.shift()` stole xs's stake, `y = (c.s = t); c.s = u`
        // left y naming the freed t.
        Expr::Ternary { .. } | Expr::Nullish { .. } | Expr::Assign { .. } => {
            !ctx.owned_member_reads.contains(&value)
        }
        Expr::BinOp {
            op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
            ..
        } => !ctx.owned_member_reads.contains(&value),
        _ => false,
    };
    if needs_inc {
        ctx.emit_rc_inc(*v);
    }
}
