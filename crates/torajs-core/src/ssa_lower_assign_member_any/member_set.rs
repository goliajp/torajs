//! The tag-gated `__torajs_any_member_set` tail both `any`-receiver
//! member-write paths share: the literal-key shell and the
//! key-parameterized core. Moved out of [`super`].

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Shared tail of both `any`-receiver member-write paths — the
/// tag-gated `__torajs_any_member_set` call (RFC 20260704 C4+).
/// The receiver rides an AnyValue slot so the DynObj arm's
/// realloc-on-grow relocation writes the fresh cell back; a named
/// Ident receiver then re-stores its local from the slot (the
/// Step 7d-A write-back, now covering the Any-payload path too).
/// The hint interns the compile-time member name for the RegExp
/// `lastIndex` / Arr `length` runtime arms.
pub(crate) fn emit_any_member_set(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &(impl AsRef<torajs_wtf8::Wtf8> + ?Sized),
    tag_op: Operand,
    val_op: Operand,
    obj_ident: &Option<String>,
    recv_owned: bool,
) {
    let field: &torajs_wtf8::Wtf8 = field.as_ref();
    let key_str = ctx.intern_string_literal(field);
    let hint = if field == "length" {
        torajs_rc::ANY_WPROP_ARR_LENGTH
    } else {
        field
            .as_str()
            .and_then(torajs_rc::any_regexp_prop_id)
            .unwrap_or(-1)
    };
    // §7.3.32 PrivateSet — a `__priv_`-mangled name selects the
    // brand-gated write channel (read twin: `emit_member_fallback`):
    // an undeclared brand throws TypeError instead of installing an
    // expando. Statically selected, so the ordinary write path pays
    // nothing; dynamic-string keys (the dyn-key callers below) never
    // carry private semantics and keep the base kernel.
    let kernel = if field.starts_with("__priv_") {
        ctx.intrinsics.any_member_set_priv
    } else {
        ctx.intrinsics.any_member_set
    };
    emit_any_member_set_dyn(
        ctx, kernel, obj_val, key_str, hint, tag_op, val_op, obj_ident, recv_owned,
    );
}

/// Key-parameterized core of [`emit_any_member_set`] — the L3b #13
/// dynamic-string-key index write reuses it with a runtime Str cell
/// key and no compile-time hint (a dynamic key that names
/// `lastIndex` / `length` misses their hint arms — recorded
/// boundary, loud not silent).
///
/// `recv_owned` — the receiver operand is a fresh owned box (an
/// inline `(expr as any)` cast, a member-read chain) rather than a
/// borrow of a binding's stake. The kernel reads the receiver
/// through the slot without consuming it, and an owned box used to
/// have NO release site here at all. That is a leak, and worse than
/// one: the stranded +1 sits on the receiver during the at-exit
/// cycle drain, so a prototype cycle that a clean run collects as
/// ONE white group gets cut in two — the first half's teardown drops
/// the second half's targets to zero early, and the second half then
/// decs through freed memory. `(TypeError.prototype as any).x = 1`
/// was a one-line reproduction: rc underflow on %Error.prototype%.
/// The drop reads the slot (not the pre-call operand) because the
/// dynobj-resize arm writes the relocated cell back into it.
// operands of one kernel call plus the receiver ownership facts
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_any_member_set_dyn(
    ctx: &mut LowerCtx<'_>,
    kernel: crate::ssa::FuncId,
    obj_val: Operand,
    key_str: crate::ssa::ValueId,
    hint: i64,
    tag_op: Operand,
    val_op: Operand,
    obj_ident: &Option<String>,
    recv_owned: bool,
) {
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(obj_val, Operand::Value(slot), 0));
    let args = vec![
        Operand::Value(slot),
        Operand::Value(key_str),
        tag_op,
        val_op,
        Operand::ConstI64(hint),
    ];
    // The ordinary kernel has a sloppy-goal twin; the private-brand
    // one does not (a brand miss is a TypeError in every goal).
    if kernel == ctx.intrinsics.any_member_set {
        let soft = ctx.intrinsics.any_member_set_soft;
        crate::ssa_lower_set_flavor::emit_any_set(ctx, kernel, soft, args);
    } else {
        let cur_block = ctx.cur_block;
        ctx.f.append_void(cur_block, InstKind::Call(kernel, args));
    }
    // P3.attribute-flag-tracking + the tag-gate boundary TypeErrors.
    ctx.emit_throw_check(None);
    if recv_owned {
        // An owned receiver has no binding to write back to; its box
        // stake releases here instead (see the fn doc). Sits after the
        // throw check exactly as the write-back does — the throw path
        // skips both, which is the pre-existing posture of this slot.
        let cur_block = ctx.cur_block;
        let fresh = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        );
        ctx.emit_drop_value(Operand::Value(fresh), Type::Any);
        return;
    }
    write_back_recv(ctx, slot, obj_ident);
}

/// An `any` local receiver takes back the slot a write kernel read it
/// through (a dynobj resize leaves the relocated cell there).
pub(crate) fn write_back_recv(
    ctx: &mut LowerCtx<'_>,
    slot: crate::ssa::ValueId,
    obj_ident: &Option<String>,
) {
    let Some(name) = obj_ident else { return };
    let Some(info) = ctx.locals.get(name).copied() else {
        return;
    };
    if !matches!(info.ty, Type::Any) {
        return;
    }
    let cur_block = ctx.cur_block;
    let fresh = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(fresh), Operand::Value(info.slot), 0),
    );
}
