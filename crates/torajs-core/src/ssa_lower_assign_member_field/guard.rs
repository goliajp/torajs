//! The frozen / redefined-field guard a typed struct field store runs
//! before it writes — moved out of
//! [`super`] at the 500-line limit.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

mod absent;

/// A generator machine writing or reading its own state: those fields
/// are the generator's internal slots (§27.5.2), which no integrity
/// level reaches (§7.3.16 walks own property keys), so
/// `Object.freeze(it); it.next()` still advances. The machine reaches
/// them only through the bases the desugar mints — its `this`, the
/// `__gself` alias, a nested machine's frame handle.
pub(crate) fn generator_own_slot(ctx: &LowerCtx, obj: ExprId) -> bool {
    let minted_base = matches!(ctx.ast.get_expr(obj), Expr::Ident(n)
        if n == "__this"
            || n == crate::ast::GEN_SELF_ALIAS
            || n.starts_with("__genouter_"));
    minted_base
        && matches!(ctx.expr_types.get(&obj),
            Some(crate::check::Type::ClassRef(c)) if ctx.ast.generator_machine_classes.contains(c))
}

/// Frozen / redefined-field guard before a struct field store of `v`.
///
/// Rotation 470 — the two bits are read INLINE (the u16 `flags` at +6
/// is the high half of the u32 at +4, whose low half is `type_tag`).
/// An unfrozen, never-redefined instance — every instance in a hot
/// ctor loop — pays one load, one and, one not-taken branch.
///
/// A hit runs the whole §10.1.9.2 [[Set]] on the any lane
/// (`__torajs_obj_field_set_exotic`): the field may have been frozen,
/// demoted to non-writable, deleted, shadowed, or turned into an
/// accessor whose setter must run instead of the store. A refusal
/// raises in strict code and answers 0 in sloppy code, which skips the
/// store (and releases a discarded assignment's stake, [`refusal`]). A scalar the leg accepted is already where it belongs. A heap
/// value still takes the slot afterwards: the slot's stake is what keeps
/// the assignment's value alive for its consumer, and the slot is the
/// value the leg wrote, or one no own face reads behind an accessor.
///
/// Leaves `cur_block` on the store block. `Some(done)` when a guard was
/// emitted: the caller ends its store block with a branch there and
/// continues from it.
pub(crate) fn emit_field_writable_guard(
    ctx: &mut LowerCtx,
    obj: ExprId,
    obj_val: Operand,
    (field, slot_ty): (&str, Type),
    v: Operand,
) -> Option<BlockId> {
    if !store_guarded(ctx, obj, field, slot_ty) {
        return None;
    }
    let hdr_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, obj_val, 4),
        Type::I32,
        None,
    );
    let absent = match ctx.ast.mints_absent_set {
        true => torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD,
        false => 0,
    };
    let guard_bits =
        ((torajs_rc::FLAG_FROZEN | torajs_rc::FLAG_OBJ_EXOTIC_FIELD | absent) as u32) << 16;
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr_word),
            Operand::ConstI32(guard_bits as i32),
        ),
        Type::I32,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(masked), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let check_blk = ctx.f.add_block();
    let store_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: check_blk,
            else_blk: store_blk,
        },
    );
    ctx.cur_block = check_blk;
    if absent != 0 {
        absent::clear_absent(ctx, (obj_val, Operand::Value(masked)), field, store_blk);
    }
    let v_ty = ctx.operand_ty(&v);
    // the leg takes its own stake in the payload; the caller's stays
    // with the slot store (or the parked throw temp)
    let (tag, val) = match (v_ty, v) {
        (Type::Ptr, Operand::Value(_)) => (Operand::ConstI64(4), v),
        _ => ctx.box_to_tag_value(v),
    };
    let name_str = ctx.intern_string_literal(field);
    let strict = !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx);
    let set = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_field_set_exotic,
            vec![
                obj_val,
                Operand::Value(name_str),
                tag,
                val,
                Operand::ConstI64(i64::from(strict)),
            ],
        ),
        Type::I64,
        None,
    );
    // forced: the refusal must divert before the store below
    ctx.emit_throw_check(None);
    let cb = ctx.cur_block;
    if v_ty.is_copy() {
        ctx.f.set_term(cb, Terminator::Br(done_blk));
    } else {
        let taken = ctx.f.append_inst(
            cb,
            InstKind::ICmp(IPred::Ne, Operand::Value(set), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let refused_blk = refusal(ctx, obj, field, (v, v_ty), done_blk);
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(taken),
                then_blk: store_blk,
                else_blk: refused_blk,
            },
        );
    }
    ctx.cur_block = store_blk;
    Some(done_blk)
}

/// Whether a store into `obj.field` stands behind the guard
/// [`emit_field_writable_guard`] emits: `false` when no instance this
/// store reaches can be frozen, redefined or missing the field.
pub(crate) fn store_guarded(ctx: &LowerCtx, obj: ExprId, field: &str, slot_ty: Type) -> bool {
    // a ctor's leading writes reach an instance nothing has seen yet
    // (`ssa_lower_fn::fresh_this`)
    !generator_own_slot(ctx, obj)
        && !ctx.proofs.fresh_this_recv.contains(&obj)
        && store_may_divert(ctx, field, slot_ty)
}

/// Whether some instance's `field` can be anything but a writable data
/// slot when a store reaches it. Each bit the guard tests has its
/// writers behind spellings the program censuses: `FLAG_FROZEN` behind
/// `freeze`; `FLAG_OBJ_EXOTIC_FIELD` behind a definer naming the
/// field, a property delete, or a write parking a value in the field's
/// sidecar; the absent set behind a literal or a cast that leaves a
/// member out. The exotic bit is per instance, not per field: one
/// raised by another field's redefinition leaves this one a plain slot.
fn store_may_divert(ctx: &LowerCtx, field: &str, slot_ty: Type) -> bool {
    let names = &ctx.ast.accessor_field_names;
    ctx.ast.mints_absent_set
        || names.may_freeze_or_delete()
        || names.may(field)
        || names.may_shadow(field, slot_ty == Type::F64)
}

/// Whether the assignment storing into `obj.field` is an expression
/// statement (or a `for` update), alone or as the last operand of a
/// comma, whose value nothing reads.
fn discarded(ctx: &LowerCtx, obj: ExprId, field: &str) -> bool {
    let Some(mut e) = ctx.discarded_stmt_expr else {
        return false;
    };
    while let Expr::Sequence { right, .. } = ctx.ast.get_expr(e) {
        e = *right;
    }
    matches!(ctx.ast.get_expr(e), Expr::Assign { target, .. }
        if matches!(ctx.ast.get_expr(*target),
            Expr::Member { obj: o, name } if *o == obj && name == field))
}

/// Where a sloppy refusal of the heap value `v` goes: the slot did not
/// take the assignment's stake, so when nothing reads the assignment's
/// value (an expression statement) the stake goes here; a read value
/// keeps it as its consumer's ([`mint_read_stake`]).
fn refusal(
    ctx: &mut LowerCtx,
    obj: ExprId,
    field: &str,
    (v, v_ty): (Operand, Type),
    done_blk: BlockId,
) -> BlockId {
    if !discarded(ctx, obj, field) {
        return done_blk;
    }
    let blk = ctx.f.add_block();
    let saved = ctx.cur_block;
    ctx.cur_block = blk;
    ctx.emit_drop_value(v, v_ty);
    let end = ctx.cur_block;
    ctx.f.set_term(end, Terminator::Br(done_blk));
    ctx.cur_block = saved;
    blk
}

/// After a guarded store of the heap value `v` whose value the
/// assignment `assign` answers to a reader: a sloppy refusal left the
/// assignment's stake unspent, and the join cannot tell the arms
/// apart, so the stored arm mints the reader a stake of its own and
/// the assignment answers owned on both (the Ident-target shape,
/// `ssa_lower_assign_ident`). A strict refusal throws, and the throw
/// temp releases the stake, so strict code keeps the plain borrow.
pub(crate) fn mint_read_stake(
    ctx: &mut LowerCtx,
    (assign, obj, field): (ExprId, ExprId, &str),
    v: Operand,
    done: Option<BlockId>,
) {
    if done.is_none()
        || !ctx.operand_ty(&v).is_refcounted()
        || !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx)
        || discarded(ctx, obj, field)
    {
        return;
    }
    ctx.emit_rc_inc(v);
    ctx.owned_member_reads.insert(assign);
}

/// Close the store block [`emit_field_writable_guard`] left the caller
/// on, and continue at its join.
pub(crate) fn finish_guarded_store(ctx: &mut LowerCtx, done: Option<BlockId>) {
    if let Some(done) = done {
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(done));
        ctx.cur_block = done;
    }
}
