//! `#x in o` — the private-name brand check the parser spells as the
//! `__torajs_priv_in_op` synthetic. Moved out of [`super`] whole.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// ES2022 §13.10 ergonomic brand check —
/// `__torajs_priv_in_op(key, obj[, "C", __this])`, the parser's
/// synthetic for `#x in o` (rotation 297). The key arrives as the
/// resolved `__priv_<C>__<x>` String literal (compiler-minted,
/// interned directly — never a runtime value); the rhs boxes to Any
/// and the `__torajs_any_member_priv_has` kernel answers the
/// own-brand probe, throwing the §13.10.1 step-5 TypeError for a
/// non-Object rhs.
///
/// A unit class's element (RFC 20260917-class-eval-unit K3) belongs
/// to one evaluation: a rhs carrying the row still answers false
/// unless its hidden evaluation slot holds what the running
/// instance's does. The slot is read only past a true probe — the
/// probe is what establishes the rhs is an instance with that slot.
pub(crate) fn try_lower_priv(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != crate::ast::private_brand::PRIV_IN_OP || !(2..=4).contains(&args.len()) {
        return None;
    }
    let Expr::String(key) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let key = key.clone();
    let key_str = ctx.intern_string_literal(&key);
    let obj_op = ctx.lower_expr(args[1]);
    let obj_ty = ctx.operand_ty(&obj_op);
    let boxed = if matches!(obj_ty, Type::Any) {
        obj_op
    } else {
        // Borrow'd NaN-box (no rc traffic) — same face the `in`
        // struct arm takes.
        ctx.box_to_any(obj_op)
    };
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_priv_has,
            vec![boxed, Operand::Value(key_str)],
        ),
        Type::Bool,
        None,
    );
    // Kernel arms the non-Object TypeError and still returns a bool
    // — same pending-throw contract as the `in` kernels.
    ctx.emit_throw_check(None);
    let r = match args {
        [_, _, class, this] => same_evaluation(ctx, r, &obj_op, *class, *this),
        // A STATIC element (`ast::private_brand_static_in`): the rhs
        // has it only when it IS the evaluation's class object, which
        // the third operand (the static `this`) names. Both sides are
        // boxed cells by then, so identity is the word compare.
        [_, _, unit] => {
            let unit_op = ctx.lower_expr(*unit);
            let same = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, boxed, unit_op),
                Type::Bool,
                None,
            );
            let both = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    crate::ssa::BinOp::And,
                    Operand::Value(r),
                    Operand::Value(same),
                ),
                Type::Bool,
                None,
            );
            Operand::Value(both)
        }
        _ => Operand::Value(r),
    };
    ctx.release_owned_temp(args[1], &obj_op);
    Some(r)
}

/// `has && (unit slot of obj == unit slot of this)`, the slot read on
/// the true arm only.
fn same_evaluation(
    ctx: &mut LowerCtx<'_>,
    has: ValueId,
    obj_op: &Operand,
    class: ExprId,
    this: ExprId,
) -> Operand {
    let Expr::String(class) = ctx.ast.get_expr(class) else {
        return Operand::Value(has);
    };
    let class = class.to_string_lossy_owned();
    let Some(off) = crate::ssa_lower_call_class_synth_private_brand::unit_slot_off(ctx, &class)
    else {
        return Operand::Value(has);
    };
    let slot = ctx.alloca_in_entry(Type::Bool, Some("__priv_in_unit"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(has), Operand::Value(slot), 0),
    );
    let probe = ctx.f.add_block();
    let merge = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(has),
            then_blk: probe,
            else_blk: merge,
        },
    );
    ctx.cur_block = probe;
    let this_op = ctx.lower_expr(this);
    let mine = crate::ssa_lower_call_class_synth_private_brand::unit_word(ctx, &this_op, off);
    let theirs = crate::ssa_lower_call_class_synth_private_brand::unit_word(ctx, obj_op, off);
    let same =
        crate::ssa_lower_call_class_synth_private_brand::same_unit(ctx, mine, theirs, &class);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(same, Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(slot), 0),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}
