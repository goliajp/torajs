//! The calls into the runtime walk: an any-lane value, or a hook's
//! answer serialized as a value.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The runtime walk over an any-lane value. Under a gap it takes the
/// gap entry, which also receives the nesting level so an any-typed
/// member of a statically unfolded composite keeps indenting from
/// its parent's level instead of restarting at zero.
pub(crate) fn emit_any_walk(
    ctx: &mut LowerCtx,
    val_op: Operand,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    let v = match gap {
        Some(g) => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.anyv_json_stringify_gap,
                vec![val_op, g, Operand::ConstI64(depth as i64)],
            ),
            Type::Str,
            None,
        ),
        None => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_json_stringify, vec![val_op]),
            Type::Str,
            None,
        ),
    };
    Operand::Value(v)
}

/// [`emit_any_walk`] for a value under the property `key` of a
/// composite the compiler unfolds: the value's `toJSON` hook is
/// called with `key` (§25.5.2.3 step 2), not the root's empty one.
/// The nesting level counts only under a gap, as for the root entries.
pub(crate) fn emit_keyed_walk(
    ctx: &mut LowerCtx,
    val_op: Operand,
    key: Operand,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    let (g, depth) = match gap {
        Some(g) => (g, depth as i64),
        None => (Operand::ConstPtrNull, 0),
    };
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.json_stringify_keyed,
            vec![val_op, g, Operand::ConstI64(depth), key],
        ),
        Type::Str,
        None,
    ))
}

/// [`emit_any_walk`]'s value-only twin — §25.5.2.3 with step 2 already
/// done. The struct lane's `toJSON` road calls the hook in the
/// compiler, so its answer is serialized as a value; running the whole
/// property operation over it again applies the hook a second time.
/// A gapless call passes a NULL gap rather than taking the other
/// entry, because the value entry is the one that skips step 2 and
/// there is no gapless spelling of it.
pub(crate) fn emit_any_walk_value(
    ctx: &mut LowerCtx,
    val_op: Operand,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    let g = gap.unwrap_or(Operand::ConstPtrNull);
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.json_stringify_value,
            vec![val_op, g, Operand::ConstI64(depth as i64)],
        ),
        Type::Str,
        None,
    ))
}
