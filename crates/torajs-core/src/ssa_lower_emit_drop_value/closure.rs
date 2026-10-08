//! The closure drop, split out of `ssa_lower_emit_drop_value`.

use super::*;

/// `Type::Closure` drop — rc-gated release: dec the env header's
/// refcount inline and only on hit-zero load the synthesized drop fn
/// ptr from `CLOSURE_DROP_FN_OFF` and indirect-call it. Mirrors
/// `__torajs_value_drop_heap`'s `Tag::Closure` arm — the synthesized
/// `__env_drop_*` body carries no rc gate of its own, so the dec gate
/// lives at every release site.
///
/// RFC 20260705 chunk 553 — the pre-553 direct finalizer call assumed
/// typed sites own the env's single reference. A closure passed
/// through a `__cls` param and returned (`keep(f) { return f }`)
/// legitimately carries rc > 1 (the return retain + the caller's arg
/// temp); the unconditional finalize freed the shared env from under
/// the live binding and the next alloc aliased it.
///
/// Chunk 738 — NULL guard mirrors `emit_drop_obj`: a nullable
/// closure slot (`let h: (() => T) | null`) legitimately holds the
/// in-band null sentinel; the reassign drop-old and scope-close
/// paths must not rc-dec through it. RFC 20260710 C2b upgraded the
/// guard to nullish (NULL or the generic undefined cell).
pub(super) fn emit_drop_closure(ctx: &mut LowerCtx, val: Operand) {
    let flags_blk = ctx.f.add_block();
    let dec_blk = ctx.f.add_block();
    let skip = ctx.f.add_block();
    let skip_check = nullish_skip_cond(ctx, &val);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: skip_check,
            then_blk: skip,
            else_blk: flags_blk,
        },
    );
    // An immortal static cell (ns-static / reified builtin-method —
    // FLAG_STATIC_LITERAL) skips the whole dec: its drop_fn slot is
    // 0, so letting the inline dec reach 0 jumps to address zero
    // (`Object.getPrototypeOf(Math.max)` crashed exactly there — the
    // non-ident arg's temp release was the first consumer to inline-
    // drop one of these cells). Header +4 packs tag:u16 | flags:u16.
    ctx.cur_block = flags_blk;
    let tag_flags = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, val, 4),
        Type::I32,
        None,
    );
    let static_bit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(tag_flags),
            Operand::ConstI32((torajs_rc::FLAG_STATIC_LITERAL as i32) << 16),
        ),
        Type::I32,
        None,
    );
    let is_static = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(static_bit), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_static),
            then_blk: skip,
            else_blk: dec_blk,
        },
    );
    ctx.cur_block = dec_blk;
    let rc_new = ctx.emit_rc_dec_inline(val);
    let is_zero = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, rc_new, Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let drop_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_zero),
            then_blk: drop_blk,
            else_blk: after,
        },
    );
    ctx.cur_block = drop_blk;
    let drop_fn_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Ptr, val, CLOSURE_DROP_FN_OFF),
        Type::Ptr,
        None,
    );
    let drop_void_sig = intern_fn_sig(ctx.fn_sigs, vec![Type::Ptr], Type::Void);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::CallIndirect(drop_void_sig, Operand::Value(drop_fn_ptr), vec![val]),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    ctx.f.set_term(skip, Terminator::Br(after));
    ctx.cur_block = after;
}
