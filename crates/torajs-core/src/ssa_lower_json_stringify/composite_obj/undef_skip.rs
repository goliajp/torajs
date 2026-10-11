//! The step 8.b undefined probe of one serialized field, moved out of
//! [`super`] whole.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use torajs_wtf8::Wtf8;

/// The undefined-only probe of one field (NULL keeps the key, prints
/// null): §25.5.2.4 step 8.b skips the whole `<sep>"key":<val>`
/// segment. Answers the merge block the caller closes into, with
/// `cur_block` left on the not-undefined arm — or `None` when the
/// slot type can never spell `undefined` and the emit runs straight.
pub(super) fn emit_undef_skip(
    ctx: &mut LowerCtx,
    fty: &Type,
    fname: &Wtf8,
    field_v: &Operand,
) -> Option<crate::ssa::BlockId> {
    // Undefined-only probe (NULL keeps the key, prints null):
    // §25.5.2.4 step 8.b skips the whole `<sep>"key":<val>`
    // segment. Str slots probe the Str sentinel; refcounted
    // pointer slots (RFC 20260710 C2b) probe the generic
    // Tag::Undefined cell — without it the field recursion
    // would walk the bare oddball header as a live cell.
    let undef_probe = match fty {
        Type::Str => Some(ctx.intrinsics.str_is_undef),
        t if t.spells_undef_with_generic_cell() => Some(ctx.intrinsics.is_undef_cell),
        _ => None,
    };
    // A `number` field some write handed the F64 undefined sentinel
    // (a generator step's completion value, rotation 640): the bits
    // compare, gated by the same field-name set every other consumer
    // of the sentinel reads (`undef_f64_fields`), so a field nothing
    // ever fed a sentinel keeps the plain emit.
    let f64_undef = *fty == Type::F64 && ctx.undefable_f64_fields.contains(fname);
    if undef_probe.is_some() || f64_undef {
        let is_undef = if let Some(probe_fid) = undef_probe {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(probe_fid, vec![*field_v]),
                Type::Bool,
                None,
            )
        } else {
            let bits = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(*field_v),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(
                    IPred::Eq,
                    Operand::Value(bits),
                    Operand::ConstI64(
                        crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64,
                    ),
                ),
                Type::Bool,
                None,
            )
        };
        let emit_blk = ctx.f.add_block();
        let merge = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: merge,
                else_blk: emit_blk,
            },
        );
        ctx.cur_block = emit_blk;
        Some(merge)
    } else {
        None
    }
}
