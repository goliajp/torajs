//! Composite-type arms (`Type::Arr` / `Type::Obj`) of
//! [`super::lower`], split out 2026-07-03 (fn-debt decomp). Bodies
//! verbatim from the pre-split match arms; element / field recursion
//! goes back through `super::lower`.

use crate::ssa::{ArrId, BinOp as SsaBinOp, IPred, InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};

/// `[<e0>,<e1>,...]` — concat-loop accumulator over
/// `arr_layouts[arr_id]`-typed slots (see module doc of the parent).
///
/// A NULL Arr slot (exec/match miss result, `Nullable<Arr>`) is JS
/// `null` and stringifies to `"null"` per §25.5.2 — the walk below
/// is gated behind a null branch (pre-fix the `len` load
/// dereferenced NULL, SIGSEGV; same family as the 642
/// `json_quote_str` NULL arm on the Str lane).
pub(super) fn lower_arr(ctx: &mut LowerCtx, val_op: Operand, arr_id: ArrId) -> Operand {
    let out_slot = ctx.alloca_in_entry(Type::Str, Some("__json_arr_out"));
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, val_op.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let walk_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: walk_blk,
        },
    );
    ctx.cur_block = null_blk;
    let null_str = ctx.intern_string_literal("null");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(null_str), Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = walk_blk;
    let walked = lower_arr_walk(ctx, val_op, arr_id);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(walked, Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// The non-null walk body of [`lower_arr`] (verbatim pre-655 arm).
fn lower_arr_walk(ctx: &mut LowerCtx, val_op: Operand, arr_id: ArrId) -> Operand {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let arr_ptr = match val_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let lbrack = ctx.intern_string_literal("[");
    let rbrack = ctx.intern_string_literal("]");
    let comma = ctx.intern_string_literal(",");
    let acc = ctx.alloca_in_entry(Type::Str, Some("__json_arr"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lbrack), Operand::Value(acc), 0),
    );
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__json_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let in_bounds = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(in_bounds),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    let need_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(i_now), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let sep_blk = ctx.f.add_block();
    let no_sep_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(need_sep),
            then_blk: sep_blk,
            else_blk: no_sep_blk,
        },
    );
    ctx.cur_block = sep_blk;
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let with_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_now), Operand::Value(comma)],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_sep), Operand::Value(acc), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(no_sep_blk));
    ctx.cur_block = no_sep_blk;
    let (off_base, off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(i_now), 3, false);
    let elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, off_base.clone(), off),
        elem_ty,
        None,
    );
    let elem_str = super::lower(ctx, Operand::Value(elem), elem_ty);
    let acc_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let with_elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_now2), elem_str],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_elem), Operand::Value(acc), 0),
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    let acc_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_final), Operand::Value(rbrack)],
        ),
        Type::Str,
        None,
    );
    Operand::Value(result)
}

/// `{"k":v,...}` — compile-time field unfold from
/// `struct_layouts[sid]`; primitive-only layouts take the `__torajs_jsb_*`
/// builder fast path, everything else the str_concat chain.
pub(super) fn lower_obj(ctx: &mut LowerCtx, val_op: Operand, sid: StructId) -> Operand {
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
        let v2 = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.str_concat,
                vec![Operand::Value(v1), Operand::Value(colon)],
            ),
            Type::Str,
            None,
        );
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
    Operand::Value(result)
}
