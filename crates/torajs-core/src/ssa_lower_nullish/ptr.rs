//! `lhs ?? rhs` over a pointer-shaped lhs — the generic lane of
//! [`crate::ssa_lower_nullish`]: branch on the lhs being null or its
//! slot type's undefined sentinel, the nullish branch lowering rhs.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    lhs_op: Operand,
    lhs_ty: Type,
    lhs: ExprId,
    rhs: ExprId,
) -> Operand {
    // Rotation 507 — two different class instances (`m ?? new
    // Leaf(1)`): the checker joined them to a common ancestor, so the
    // slot wears the ancestor's layout rather than the lhs's. No box
    // — both pointers carry its field prefix (`class_join_repr`).
    let lhs_ty = ctx.class_join_repr(eid).unwrap_or(lhs_ty);
    let cur_block = ctx.cur_block;
    let eq_null = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, lhs_op, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    // Chunk 786 — a pointer-shaped slot's undefined is the immortal
    // per-type sentinel cell (RFC 20260710), not NULL: `o.tag ?? d`
    // on a filled optional field compared only against NULL, so the
    // sentinel rode the non-null path and printed its cell text
    // ("undefined") instead of taking d. Mirror the truthiness
    // station: nullish = NULL or the slot type's sentinel address.
    // Slot types with no sentinel yet keep the plain null check.
    let cond = if let Some(sentinel) = ctx.str_undef_sentinel_for(lhs_ty) {
        let eq_undef = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, lhs_op, sentinel),
            Type::Bool,
            None,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                SsaBinOp::Or,
                Operand::Value(eq_null),
                Operand::Value(eq_undef),
            ),
            Type::Bool,
            None,
        )
    } else {
        eq_null
    };
    let then_blk = ctx.f.add_block();
    let else_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk,
            else_blk,
        },
    );
    ctx.cur_block = then_blk;
    ctx.proofs.enter_cond();
    let rhs_op = crate::ssa_lower_closure_canonical::lower_slot_arm(ctx, lhs_ty, rhs);
    ctx.proofs.leave_cond();
    // The slot settles once both arms are in hand: a `Str` / `Substr`
    // view pair joins at `Str`, the view arm copied out on its own
    // path (`ssa_lower_logical::join_arm`), so the lhs is stored in its branch
    // rather than ahead of the test.
    let slot_ty =
        crate::ssa_lower_logical::join_arm::join_view_slot(lhs_ty, ctx.operand_ty(&rhs_op));
    let mut then_end = ctx.cur_block;
    let (rhs_op, rhs_minted) = ctx.settle_join_arm(&mut then_end, rhs, rhs_op, slot_ty);
    let mut else_end = else_blk;
    let (lhs_v, lhs_minted) = ctx.settle_join_arm(&mut else_end, lhs, lhs_op, slot_ty);
    let res_slot = ctx.alloca_in_entry(slot_ty, Some("__nullish"));
    ctx.f.append_void(
        then_end,
        InstKind::Store(rhs_op, Operand::Value(res_slot), 0),
    );
    ctx.f.append_void(
        else_end,
        InstKind::Store(lhs_v, Operand::Value(res_slot), 0),
    );
    // Chunk 722 — the join result is uniformly OWNED: an owned
    // shape transfers its fresh reference into the slot, a borrow
    // takes +1 in its branch tail; the eid joins the owned track
    // so the consumer's release balances (pre-fix both branches
    // inc'd unconditionally while the predicate answered borrow —
    // `mk(i) ?? "z"` leaked rc=2 per iteration on discard AND let,
    // probes p722d/e 15.3MB vs 6.2MB flat).
    if slot_ty.is_refcounted() {
        if !rhs_minted && !ctx.expr_transfers_ownership(rhs) {
            ctx.emit_rc_inc_in(then_end, rhs_op);
        }
        if !lhs_minted && !ctx.expr_transfers_ownership(lhs) {
            ctx.emit_rc_inc_in(else_end, lhs_v);
        }
        ctx.owned_member_reads.insert(eid);
    }
    ctx.f.set_term(then_end, Terminator::Br(after));
    ctx.f.set_term(else_end, Terminator::Br(after));
    ctx.cur_block = after;
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(slot_ty, Operand::Value(res_slot), 0),
        slot_ty,
        None,
    );
    Operand::Value(r)
}
