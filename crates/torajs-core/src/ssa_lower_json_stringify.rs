//! `lower_json_stringify` extracted from [`crate::ssa_lower`]
//! (chunk 158).
//!
//! Pre-extract this method was 502 LOC on `LowerCtx`. Becomes a
//! free fn here; `LowerCtx::lower_json_stringify` becomes a thin
//! pub(crate) wrapper because the body recurses on itself + several
//! ssa-lower sites call it as a method.
//!
//! Dispatches on `ty`:
//!
//! - **I64** → `__torajs_i64_to_str`.
//! - **F64** → ES §25.5.2.1 SerializeJSONNumber: `!IsFinite(x)` →
//!   `"null"` (NaN / ±Infinity are invalid JSON); finite → `f64_to_str`.
//! - **Bool** → cond_br on val, alloca slot Store `"true"` / `"false"`.
//! - **Str** → `__torajs_json_quote_str` (quote + escape).
//! - **Substr** → materialize to owned Str via `substr_to_owned`,
//!   quote, then drop the intermediate Str.
//! - **Arr(arr_id)** → `[<e0>,<e1>,…]` via concat-loop accumulator
//!   over `arr_layouts[arr_id]`-typed slots; element recursion via
//!   `lower_json_stringify`. T-13.5 head-aware slot offset via
//!   `emit_arr_slot_byte_offset`.
//! - **Obj(sid)** → compile-time field unfold from
//!   `struct_layouts[sid]`. **V0.2 P14-S5 JSON builder fast path**:
//!   when every field is I64/Bool/Str, emit through `__torajs_jsb_*`
//!   (single growing Vec<u8>, amortized O(N)) instead of str_concat
//!   chain (O(N²) byte copies). Non-primitive falls back to chain.
//! - **Ptr** (S169) → `"null"` for both null and undefined (Str-only
//!   return type can't carry undefined; tracked L3b).
//! - other → panic.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(ctx: &mut LowerCtx, val_op: Operand, ty: Type) -> Operand {
    match ty {
        Type::I64 => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.i64_to_str, vec![val_op]),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        Type::F64 => {
            let is_finite = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.num_is_finite_f, vec![val_op.clone()]),
                Type::Bool,
                None,
            );
            let finite_blk = ctx.f.add_block();
            let nonfinite_blk = ctx.f.add_block();
            let after_blk = ctx.f.add_block();
            let slot = ctx.alloca_in_entry(Type::Str, Some("__json_num"));
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(is_finite),
                    then_blk: finite_blk,
                    else_blk: nonfinite_blk,
                },
            );
            ctx.cur_block = finite_blk;
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.f64_to_str, vec![val_op]),
                Type::Str,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(s), Operand::Value(slot), 0),
            );
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
            ctx.cur_block = nonfinite_blk;
            let null_str = ctx.intern_string_literal("null");
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(null_str), Operand::Value(slot), 0),
            );
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
            ctx.cur_block = after_blk;
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Str, Operand::Value(slot), 0),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        Type::Bool => {
            let true_ptr = ctx.intern_string_literal("true");
            let false_ptr = ctx.intern_string_literal("false");
            let then_blk = ctx.f.add_block();
            let else_blk = ctx.f.add_block();
            let after_blk = ctx.f.add_block();
            let slot = ctx.alloca_in_entry(Type::Str, Some("__json_bool"));
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: val_op,
                    then_blk,
                    else_blk,
                },
            );
            ctx.f.append_void(
                then_blk,
                InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
            );
            ctx.f.set_term(then_blk, Terminator::Br(after_blk));
            ctx.f.append_void(
                else_blk,
                InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
            );
            ctx.f.set_term(else_blk, Terminator::Br(after_blk));
            ctx.cur_block = after_blk;
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Str, Operand::Value(slot), 0),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        Type::Str => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.json_quote_str, vec![val_op]),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        Type::Substr => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![val_op]),
                Type::Str,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.json_quote_str, vec![Operand::Value(owned)]),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(Operand::Value(owned), Type::Str);
            Operand::Value(v)
        }
        Type::Arr(arr_id) => {
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
            let off = ctx.emit_arr_slot_byte_offset(
                Operand::Value(arr_ptr),
                Operand::Value(i_now),
                3,
                false,
            );
            let elem = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off),
                elem_ty,
                None,
            );
            let elem_str = ctx.lower_json_stringify(Operand::Value(elem), elem_ty);
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
        Type::Obj(sid) => {
            let layout = ctx.struct_layouts[sid.0 as usize].clone();
            let obj_ptr = match val_op {
                Operand::Value(v) => v,
                _ => unreachable!(),
            };
            let primitive_only = layout
                .iter()
                .all(|(_, fty)| matches!(fty, Type::I64 | Type::Bool | Type::Str));
            if primitive_only {
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
                for (i, (fname, fty)) in layout.iter().enumerate() {
                    if i > 0 {
                        ctx.f.append_void(
                            ctx.cur_block,
                            InstKind::Call(
                                ctx.intrinsics.jsb_push_byte,
                                vec![sb_op.clone(), Operand::ConstI64(b',' as i64)],
                            ),
                        );
                    }
                    let mut key_emit = String::with_capacity(fname.len() + 3);
                    key_emit.push('"');
                    key_emit.push_str(fname);
                    key_emit.push_str("\":");
                    let key_str = ctx.intern_string_literal(&key_emit);
                    ctx.f.append_void(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.jsb_push_str_raw,
                            vec![sb_op.clone(), Operand::Value(key_str)],
                        ),
                    );
                    let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
                    let field_v = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
                        *fty,
                        None,
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
                        Type::Str => {
                            ctx.f.append_void(
                                ctx.cur_block,
                                InstKind::Call(
                                    ctx.intrinsics.jsb_push_str_quoted,
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
                return Operand::Value(result);
            }
            let lbrace = ctx.intern_string_literal("{");
            let rbrace = ctx.intern_string_literal("}");
            let comma = ctx.intern_string_literal(",");
            let colon = ctx.intern_string_literal(":");
            let mut acc = Operand::Value(lbrace);
            for (i, (fname, fty)) in layout.iter().enumerate() {
                if i > 0 {
                    let v = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.str_concat, vec![acc, Operand::Value(comma)]),
                        Type::Str,
                        None,
                    );
                    acc = Operand::Value(v);
                }
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
                        vec![acc, Operand::Value(key_quoted)],
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
                let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
                let field_v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
                    *fty,
                    None,
                );
                let field_str = ctx.lower_json_stringify(Operand::Value(field_v), *fty);
                let v3 = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.str_concat,
                        vec![Operand::Value(v2), field_str],
                    ),
                    Type::Str,
                    None,
                );
                acc = Operand::Value(v3);
            }
            let result = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_concat, vec![acc, Operand::Value(rbrace)]),
                Type::Str,
                None,
            );
            Operand::Value(result)
        }
        Type::Ptr => {
            let p = ctx.intern_string_literal("null");
            Operand::Value(p)
        }
        other => panic!("ssa-lower: JSON.stringify on type {other:?} not yet supported"),
    }
}
