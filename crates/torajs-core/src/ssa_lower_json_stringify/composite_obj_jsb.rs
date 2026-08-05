//! `lower_obj`'s primitive-only fast lane — split out when the §20.5
//! error-attribute route pushed `composite_obj.rs` past the 500-line
//! limit. The two lanes were always independent: this one writes into
//! a single growing `__torajs_jsb_*` buffer while the concat lane it
//! left behind threads a slot-held accumulator, and they share
//! nothing but the caller's layout slice.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

/// jsb-builder fast path (primitive-only layouts): single growing
/// buffer, runtime `pending_sep` comma protocol, Str fields fused
/// into `jsb_push_field_str` (undefined → key skip).
pub(super) fn lower_obj_jsb(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    layout: &[(String, Type)],
) -> Operand {
    let initial_cap: u64 = 2 + layout
        .iter()
        .map(|(name, _)| (name.len() + 8) as u64)
        .sum::<u64>();
    let sb = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.jsb_new,
            vec![Operand::ConstI64(initial_cap as i64)],
        ),
        Type::Ptr,
        None,
    );
    let sb_op = Operand::Value(sb);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.jsb_push_byte,
            vec![sb_op.clone(), Operand::ConstI64(b'{' as i64)],
        ),
    );
    // Chunk 658 — runtime comma protocol: an undefined Str field
    // skips its key per §25.5.2.4 step 8.b, so the `,` decision
    // is "has any field been emitted" (builder `pending_sep`),
    // not the compile-time `i > 0`.
    for (i, (fname, fty)) in layout.iter().enumerate() {
        let mut key_emit = String::with_capacity(fname.len() + 3);
        key_emit.push('"');
        key_emit.push_str(fname);
        key_emit.push_str("\":");
        let key_str = ctx.intern_string_literal(&key_emit);
        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
        let field_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
            *fty,
            None,
        );
        if *fty == Type::Str {
            // Sentinel probe + sep + key + quoted val fused in
            // the runtime helper (skips everything on undefined).
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.jsb_push_field_str,
                    vec![
                        sb_op.clone(),
                        Operand::Value(key_str),
                        Operand::Value(field_v),
                    ],
                ),
            );
            continue;
        }
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.jsb_begin_field, vec![sb_op.clone()]),
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.jsb_push_str_raw,
                vec![sb_op.clone(), Operand::Value(key_str)],
            ),
        );
        match fty {
            Type::I64 => {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.jsb_push_i64,
                        vec![sb_op.clone(), Operand::Value(field_v)],
                    ),
                );
            }
            Type::Bool => {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.jsb_push_bool,
                        vec![sb_op.clone(), Operand::Value(field_v)],
                    ),
                );
            }
            _ => unreachable!("primitive_only gate"),
        }
    }
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.jsb_push_byte,
            vec![sb_op.clone(), Operand::ConstI64(b'}' as i64)],
        ),
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.jsb_finalize, vec![sb_op]),
        Type::Str,
        None,
    );
    Operand::Value(result)
}
