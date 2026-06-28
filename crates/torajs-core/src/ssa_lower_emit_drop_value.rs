//! `emit_drop_value` extracted from [`crate::ssa_lower`] (chunk 159).
//!
//! Pre-extract this method was 358 LOC on `LowerCtx`. Becomes a
//! free fn here; `LowerCtx::emit_drop_value` becomes a thin
//! pub(crate) wrapper because the body recurses on itself (Obj
//! field walk) + many ssa-lower sites call it as a method.
//!
//! Per-type drop dispatch — emit the IR for releasing one owned
//! value at scope close. Bodies preserved verbatim:
//!
//! - **Str / Substr** → simple `str_drop` / `substr_drop` call
//!   (refcount + parent chain handled in the runtime helper).
//! - **Obj(sid)** — V3-05 self-referential class layouts: first
//!   inline frame inserts `sid` into `drop_inline_stack`;
//!   recursive children of the same sid route through the runtime
//!   tag-dispatched `value_drop_heap` instead. Body inlines
//!   `if (val != null) { if (--rc == 0) { walk_fields; free } }`:
//!   - NULL guard (nullable Obj patterns)
//!   - `emit_rc_dec_inline` returns new rc
//!   - T-26.C: class-sid hits `cycle_buffer` when rc didn't reach 0
//!     (Bacon-Rajan collector cycle root); buffered flag in runtime
//!     prevents dup-buffering
//!   - On rc==0: T-26 clear WeakRefs for class instances,
//!     recurse on owned fields, T-26.B `cycle_unbuffer` scrub
//!     (skipped for anon-structs to keep generic-pair-1m fast)
//!   - Sized `obj_drop_sized(val, OBJ_HEADER_SIZE + N*8)`
//! - **Arr(arr_id)** — NULL guard (regex/match no-match yields null),
//!   then per-elem-ty path: Any → `arr_drop_any` (16-byte tagged
//!   slots); refcounted elems → `emit_arr_rc_drop_range` to dec
//!   each element, then `arr_drop`.
//! - **Closure** — load drop_fn ptr from `CLOSURE_DROP_FN_OFF` and
//!   indirect-call; the synthesized drop fn (Pass 2.5) walks env
//!   captures and frees the env block.
//! - **RegExp / Date / Any / Symbol / Promise / BigInt / WeakRef /
//!   WeakMap / WeakSet / Map / Set / MapIter / ArrIter** — all
//!   route through their type-specific `__torajs_*_drop` runtime
//!   helper (rc-aware: dec, free at zero, recursive child drops as
//!   needed).
//! - **Copy** types — no-op (caller normally filters; defensive).
//! - other — panic.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{
    ARR_LEN_OFF, CLOSURE_DROP_FN_OFF, LowerCtx, OBJ_HEADER_SIZE, intern_fn_sig,
};

pub(crate) fn emit(ctx: &mut LowerCtx, val: Operand, ty: Type) {
    match ty {
        Type::Str => {
            let drop_fid = ctx.intrinsics.str_drop;
            ctx.f
                .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
        }
        Type::Substr => {
            let drop_fid = ctx.intrinsics.substr_drop;
            ctx.f
                .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
        }
        Type::Obj(sid) => {
            if ctx.drop_inline_stack.contains(&sid.0) {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.value_drop_heap, vec![val]),
                );
                return;
            }
            ctx.drop_inline_stack.insert(sid.0);
            let layout = ctx.struct_layouts[sid.0 as usize].clone();
            let dec_blk = ctx.f.add_block();
            let walk_blk = ctx.f.add_block();
            let after = ctx.f.add_block();
            let null_check = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, val, Operand::ConstPtrNull),
                Type::Bool,
                None,
            );
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(null_check),
                    then_blk: after,
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
            let is_class_sid = ctx
                .ast
                .class_parents
                .keys()
                .any(|cn| matches!(ctx.aliases.get(cn), Some(Type::Obj(s)) if s.0 == sid.0));
            let buffer_blk = if is_class_sid {
                ctx.f.add_block()
            } else {
                after
            };
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(is_zero),
                    then_blk: walk_blk,
                    else_blk: buffer_blk,
                },
            );
            if is_class_sid {
                ctx.cur_block = buffer_blk;
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.cycle_buffer, vec![val]),
                );
                ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
            }
            ctx.cur_block = walk_blk;
            if is_class_sid {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.weakref_target_dying, vec![val]),
                );
            }
            for (i, (_, fty)) in layout.iter().enumerate() {
                if fty.is_copy() {
                    continue;
                }
                let offset = OBJ_HEADER_SIZE + i as u64 * 8;
                let field_val =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::Load(*fty, val, offset), *fty, None);
                ctx.emit_drop_value(Operand::Value(field_val), *fty);
            }
            if is_class_sid {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.cycle_unbuffer, vec![val]),
                );
            }
            let obj_block_size = OBJ_HEADER_SIZE + (layout.len() as u64) * 8;
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.obj_drop_sized,
                    vec![val, Operand::ConstI64(obj_block_size as i64)],
                ),
            );
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
            ctx.cur_block = after;
            ctx.drop_inline_stack.remove(&sid.0);
        }
        Type::Arr(arr_id) => {
            let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
            let body_blk = ctx.f.add_block();
            let after = ctx.f.add_block();
            let null_check = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, val.clone(), Operand::ConstPtrNull),
                Type::Bool,
                None,
            );
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(null_check),
                    then_blk: after,
                    else_blk: body_blk,
                },
            );
            ctx.cur_block = body_blk;
            if elem_ty == Type::Any {
                let drop_fid = ctx.intrinsics.arr_drop_any;
                ctx.f
                    .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
            } else {
                if elem_ty.is_refcounted() {
                    let len_v = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(Type::I64, val.clone(), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    ctx.emit_arr_rc_drop_range(
                        val.clone(),
                        elem_ty,
                        Operand::ConstI64(0),
                        Operand::Value(len_v),
                    );
                }
                let drop_fid = ctx.intrinsics.arr_drop;
                ctx.f
                    .append_void(ctx.cur_block, InstKind::Call(drop_fid, vec![val]));
            }
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
            ctx.cur_block = after;
        }
        Type::Closure(_) => {
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
        }
        Type::RegExp => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.regex_drop, vec![val]),
            );
        }
        Type::Date => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.date_drop, vec![val]),
            );
        }
        Type::Any => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_box_drop, vec![val]),
            );
        }
        Type::Symbol => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.symbol_drop, vec![val]),
            );
        }
        Type::Promise => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.promise_drop, vec![val]),
            );
        }
        Type::BigInt => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.bigint_drop_rc, vec![val]),
            );
        }
        Type::WeakRef => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.weakref_drop, vec![val]),
            );
        }
        Type::WeakMap => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.weakmap_drop, vec![val]),
            );
        }
        Type::WeakSet => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.weakset_drop, vec![val]),
            );
        }
        Type::Map => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_drop, vec![val]),
            );
        }
        Type::Set => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_drop, vec![val]),
            );
        }
        Type::MapIter => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.map_iter_drop, vec![val]),
            );
        }
        Type::ArrIter => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_iter_drop, vec![val]),
            );
        }
        other if other.is_copy() => {}
        other => panic!("ssa-lower: no drop sequence for type {other:?}"),
    }
}
