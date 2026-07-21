//! `m.getOrInsert(key, default)` lowering — pulled out of
//! [`crate::ssa_lower_call_map_dispatch`] when the chunk-566 mint-ref
//! settles pushed that file over the 500-line hard limit (verbatim
//! move, rotation 171).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `m.getOrInsert(key, default)` per the stage-3 upsert proposal
/// (RFC 20260721 刀 6) — key/default packed like `set`, answer read
/// back through `get`-style out slots (the kernel owns both stakes
/// and rc-bumps the answered value).
pub(crate) fn emit_map_get_or_insert(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    debug_assert!(args.len() >= 2);
    let (k_tag, k_val, k_raw, _) = ctx.lower_to_tag_value_raw(args[0]);
    let (d_tag, d_val, d_raw, _) = ctx.lower_to_tag_value_raw(args[1]);
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let tag_slot = ctx.alloca(Type::I64, Some("map_goi_tag"));
    let val_slot = ctx.alloca(Type::I64, Some("map_goi_val"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.map_get_or_insert,
            vec![
                recv_op,
                k_tag,
                k_val,
                d_tag,
                d_val,
                Operand::Value(tag_slot),
                Operand::Value(val_slot),
            ],
        ),
    );
    // Chunk 566 share — settle owned-temp key/default mint refs
    // (the kernel owns the packed +1s; the hit path double-stakes
    // the default and releases its own, this is the lower's copy).
    ctx.release_owned_temp(args[0], &k_raw);
    ctx.release_owned_temp(args[1], &d_raw);
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(tag_slot), 0),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(val_slot), 0),
        Type::I64,
        None,
    );
    let box_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(box_v)
}
