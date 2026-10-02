//! The frozen / redefined-field guard a typed struct field store runs
//! before it writes — moved out of
//! [`super`] at the 500-line limit.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Frozen / redefined-field guard before a struct field store.
///
/// Rotation 470 — the two bits the helper tests are read INLINE (the
/// u16 `flags` at +6 is the high half of the u32 at +4, whose low half
/// is `type_tag`); `__torajs_obj_check_field_writable` and its throw
/// check only run on a hit. An unfrozen, never-redefined instance —
/// every instance in a hot ctor loop — pays one load, one and, one
/// not-taken branch instead of two cross-archive calls (~6% + ~3% of
/// `class-method`). Leaves `cur_block` on the store block.
pub(crate) fn emit_field_writable_guard(
    ctx: &mut LowerCtx,
    obj: ExprId,
    obj_val: Operand,
    field: &str,
) {
    // A generator machine writing its own state: those fields are the
    // generator's internal slots (§27.5.2), which no integrity level
    // reaches (§7.3.16 walks own property keys), so
    // `Object.freeze(it); it.next()` still advances. The machine
    // reaches them only through the bases the desugar mints — its
    // `this`, the `__gself` alias, a nested machine's frame handle.
    let minted_base = matches!(ctx.ast.get_expr(obj), Expr::Ident(n)
        if n == "__this"
            || n == crate::ast::GEN_SELF_ALIAS
            || n.starts_with("__genouter_"));
    if minted_base
        && matches!(ctx.expr_types.get(&obj),
            Some(crate::check::Type::ClassRef(c)) if ctx.ast.generator_machine_classes.contains(c))
    {
        return;
    }
    let hdr_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, obj_val, 4),
        Type::I32,
        None,
    );
    let guard_bits = ((torajs_rc::FLAG_FROZEN | torajs_rc::FLAG_OBJ_EXOTIC_FIELD) as u32) << 16;
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
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: check_blk,
            else_blk: store_blk,
        },
    );
    ctx.cur_block = check_blk;
    let name_str = ctx.intern_string_literal(field);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_check_field_writable,
            vec![obj_val, Operand::Value(name_str)],
        ),
    );
    // P7.4-frozen — obj_check_not_frozen now arms a real TypeError
    // (instead of process abort) when the target is frozen. Force
    // the throw-check here (intrinsic → emit_throw_check(Some) would
    // skip it) so it diverts to the try/catch or propagates BEFORE
    // the field store below — the illegal mutation must not happen.
    // Mirrors the a-2 dynobj writable=false pattern.
    ctx.emit_throw_check(None);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(store_blk));
    ctx.cur_block = store_blk;
}
