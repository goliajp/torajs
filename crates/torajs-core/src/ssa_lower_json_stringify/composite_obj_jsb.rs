//! `lower_obj`'s primitive-only fast lane — split out when the §20.5
//! error-attribute route pushed `composite_obj.rs` past the 500-line
//! limit. The two lanes were always independent: this one writes into
//! a single growing `__torajs_jsb_*` buffer while the concat lane it
//! left behind threads a slot-held accumulator, and they share
//! nothing but the caller's layout slice.

use crate::ast::PropKey;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};
use torajs_wtf8::{Wtf8, Wtf8Buf};

/// jsb-builder fast path (primitive-only layouts): single growing
/// buffer, runtime `pending_sep` comma protocol, Str fields fused
/// into `jsb_push_field_str` (undefined → key skip).
pub(super) fn lower_obj_jsb(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    layout: &[(PropKey, Type)],
) -> Operand {
    if let Some(r) = try_lower_obj_shape(ctx, obj_ptr, layout) {
        return r;
    }
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
        let key_str = ctx.intern_string_literal(&json_key_spelling(fname));
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

/// Descriptor-driven single-call lane (S1-A2 attack A1): encode the
/// whole static layout as a u7-safe blob in an interned Latin-1
/// literal and emit ONE `__torajs_jsb_stringify_shape(desc, obj)`
/// call in place of the per-field fabric above (11 calls for a
/// 4-field record). Blob format is documented on the kernel
/// (`torajs-str/src/json_shape.rs`):
///
/// ```text
/// [n_fields+1] n × [key_len+1][ty+1: 1=I64 2=Bool 3=Str][slot_idx+1][key…]
/// ```
///
/// Every scalar is stored +1 (range 1..=128) so the blob never
/// carries a NUL byte, and must stay ≤ 128 so it remains valid
/// UTF-8 for the intern surface; keys must be ASCII (the pre-quoted
/// `"k":` spelling rides in the blob verbatim). Layouts that
/// overflow any u7 or carry a non-ASCII key answer `None` and keep
/// the per-field lane.
fn try_lower_obj_shape(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    layout: &[(PropKey, Type)],
) -> Option<Operand> {
    if layout.len() >= 128 {
        return None;
    }
    let mut desc = String::with_capacity(1 + layout.len() * 12);
    desc.push((layout.len() as u8 + 1) as char);
    for (i, (fname, fty)) in layout.iter().enumerate() {
        let ty_code: u8 = match fty {
            Type::I64 => 1,
            Type::Bool => 2,
            Type::Str => 3,
            _ => return None,
        };
        // key_len counts the pre-quoted `"name":` spelling.
        let key_len = fname.len() + 3;
        if key_len >= 128 || i >= 128 || !fname.is_ascii() {
            return None;
        }
        desc.push((key_len as u8 + 1) as char);
        desc.push(ty_code as char);
        desc.push((i as u8 + 1) as char);
        desc.push('"');
        desc.push_str(fname.as_str().expect("ascii key"));
        desc.push_str("\":");
    }
    let desc_lit = ctx.intern_string_literal(&desc);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.jsb_stringify_shape,
            vec![Operand::Value(desc_lit), Operand::Value(obj_ptr)],
        ),
        Type::Str,
        None,
    );
    Some(Operand::Value(result))
}

/// `"<key>":` as §25.5.2.3 QuoteJSONString spells the key: a lone
/// surrogate becomes `\uXXXX`, everything else (well-formed, no
/// quote / backslash / control byte — the layout gate upstream keeps
/// those on the runtime `json_quote_str` lane) rides verbatim.
fn json_key_spelling(key: &Wtf8) -> Wtf8Buf {
    let mut out = Wtf8Buf::with_capacity(key.len() + 3);
    out.push_str("\"");
    for cp in key.code_points() {
        if (0xD800..=0xDFFF).contains(&cp) {
            out.push_str(&format!("\\u{cp:04x}"));
        } else {
            out.push_code_point(cp);
        }
    }
    out.push_str("\":");
    out
}
