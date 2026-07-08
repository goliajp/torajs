//! `Type::Obj` arm of [`super::lower`] — split from
//! `composite.rs` when the 666 null gate pushed it over the
//! 500-line file limit. Field recursion goes back through
//! `super::lower`; the shared null gate lives in
//! [`super::composite`].

use crate::ssa::{InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

use super::composite::with_null_gate;

/// `{"k":v,...}` — compile-time field unfold from
/// `struct_layouts[sid]`; primitive-only layouts take the `__torajs_jsb_*`
/// builder fast path, everything else the str_concat chain. The
/// unfold sits behind the shared [`with_null_gate`] — a NULL Obj
/// slot (`Nullable<Obj>` holding JS null) answers `"null"` instead
/// of dereferencing NULL on the first field load (655 arr-arm
/// mirror; the parse-reject that used to shield this lane is gone).
pub(super) fn lower_obj(ctx: &mut LowerCtx, val_op: Operand, sid: StructId) -> Operand {
    with_null_gate(ctx, &val_op.clone(), "__json_obj_out", |ctx| {
        let layout = ctx.struct_layouts[sid.0 as usize].clone();
        let obj_ptr = match val_op {
            Operand::Value(v) => v,
            _ => unreachable!(),
        };
        let primitive_only = layout
            .iter()
            .all(|(_, fty)| matches!(fty, Type::I64 | Type::Bool | Type::Str));
        if primitive_only {
            lower_obj_jsb(ctx, obj_ptr, &layout)
        } else {
            lower_obj_concat(ctx, obj_ptr, &layout)
        }
    })
}

/// jsb-builder fast path (primitive-only layouts): single growing
/// buffer, runtime `pending_sep` comma protocol, Str fields fused
/// into `jsb_push_field_str` (undefined → key skip).
fn lower_obj_jsb(
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

/// str_concat slow lane (mixed layouts): slot-held accumulator,
/// runtime `json_obj_sep` separator, per-Str-field sentinel branch
/// (undefined → the whole `<sep>"key":<val>` segment is skipped).
fn lower_obj_concat(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    layout: &[(String, Type)],
) -> Operand {
    // Chunk 658 — the accumulator lives in a slot and the `,`
    // separator is a runtime decision (`json_obj_sep`: emitted-any-
    // field ⇔ acc longer than the opening `{`): an undefined Str
    // field skips key + value + separator per §25.5.2.4 step 8.b,
    // which a compile-time `i > 0` comma cannot express.
    let lbrace = ctx.intern_string_literal("{");
    let rbrace = ctx.intern_string_literal("}");
    let colon = ctx.intern_string_literal(":");
    let acc_slot = ctx.alloca_in_entry(Type::Str, Some("__json_obj_acc"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lbrace), Operand::Value(acc_slot), 0),
    );
    for (i, (fname, fty)) in layout.iter().enumerate() {
        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
        let field_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
            *fty,
            None,
        );
        let merge_blk = if *fty == Type::Str {
            // Undefined-only probe (NULL keeps the key, prints null).
            let is_undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_is_undef, vec![Operand::Value(field_v)]),
                Type::Bool,
                None,
            );
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
        };
        let acc_now = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Str, Operand::Value(acc_slot), 0),
            Type::Str,
            None,
        );
        let acc_sep = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.json_obj_sep, vec![Operand::Value(acc_now)]),
            Type::Str,
            None,
        );
        let key_str = ctx.intern_string_literal(fname);
        let key_quoted = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.json_quote_str, vec![Operand::Value(key_str)]),
            Type::Str,
            None,
        );
        let v1 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.str_concat,
                vec![Operand::Value(acc_sep), Operand::Value(key_quoted)],
            ),
            Type::Str,
            None,
        );
        // 642 ledger — every str_concat answers a fresh Str, so each
        // consumed link releases right after its concat (json_obj_sep
        // is owned-in/owned-out and already settled acc_now; interned
        // literals no-op through the same drop).
        ctx.emit_drop_value(Operand::Value(acc_sep), Type::Str);
        ctx.emit_drop_value(Operand::Value(key_quoted), Type::Str);
        let v2 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.str_concat,
                vec![Operand::Value(v1), Operand::Value(colon)],
            ),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(Operand::Value(v1), Type::Str);
        let field_str = super::lower(ctx, Operand::Value(field_v), *fty);
        let v3 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.str_concat,
                vec![Operand::Value(v2), field_str],
            ),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(Operand::Value(v2), Type::Str);
        ctx.emit_drop_value(field_str.clone(), Type::Str);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v3), Operand::Value(acc_slot), 0),
        );
        if let Some(mb) = merge_blk {
            ctx.f.set_term(ctx.cur_block, Terminator::Br(mb));
            ctx.cur_block = mb;
        }
    }
    let acc_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc_slot), 0),
        Type::Str,
        None,
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_final), Operand::Value(rbrace)],
        ),
        Type::Str,
        None,
    );
    // 642 ledger — release the final accumulator (an all-skipped
    // object's acc is still the "{" literal: no-op drop).
    ctx.emit_drop_value(Operand::Value(acc_final), Type::Str);
    Operand::Value(result)
}
