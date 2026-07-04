//! `Expr::Index { obj, index }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-67
//! of the decomp (chunks 1-66 = ... + P8.2 accessor + struct-
//! field terminal arm).
//!
//! Lowers `xs[i]` for three receiver shapes:
//!
//! - **String indexing** (`Type::Str` / `Type::Substr`) — returns a
//!   single-char `Type::Substr` view.
//!   - **V3-18 m1.h.44** — Str routes through
//!     `__torajs_str_char_at(s, idx)` for bounds-checked indexing
//!     (direct `substr_create` trusted user idx and OOB stored
//!     garbage offsets; same fix as charAt m1.h.37).
//!   - Substr routes through
//!     `__torajs_substr_slice(v, idx, idx+1)` (resolves to root
//!     parent via runtime).
//! - **Array<Any> indexed read** (`Type::Arr(arr_id)` with
//!   `elem_ty == Type::Any`) — 16-byte slot stride dual-load
//!   `arr_get_any_tag` + `arr_get_any_value` + `any_box` packs
//!   the (tag, value) pair into a single Any-box ptr the SSA
//!   layer can carry. Per-read alloc is the trade-off vs SSA-
//!   layer pair passing; use-site fast paths (`console.log(xs[i])`)
//!   may inline direct dispatch without box (T-10.e). **P1.4**
//!   bounds-check is in the runtime helper: OOB returns
//!   `ANY_UNDEF=5` per ES spec §10.4.2.1 (pre-P1.4 inlined the
//!   `LoadDyn` at `24 + (head+i)*16` unconditionally and returned
//!   garbage / ANY_NULL).
//! - **Array<T>** (`Type::Arr(arr_id)` with concrete `elem_ty`) —
//!   `LoadDyn(elem_ty, arr_val, offset)` where offset is computed
//!   by `emit_arr_slot_byte_offset` (T-13.5 deque: `24 + (idx +
//!   head) * 8`; non-deque names take the fast path via 11-A1
//!   `arr_expr_is_non_deque` peek). Bounds-checking is deferred
//!   (currently unchecked — UB on OOB, matches bun's hot-path
//!   behaviour after JIT).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Index` match arm bottoms out here, no None fall-through).

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, obj: ExprId, index: ExprId) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    let arr_ty = ctx.operand_ty(&arr_val);
    if matches!(arr_ty, Type::Str | Type::Substr) {
        return lower_string_index(ctx, arr_val, arr_ty, index);
    }
    // Any-dynamic-access RFC (20260704) S3 — `recv[i]` where recv is
    // an `any` value: runtime dispatch (kind-aware Arr / Str /
    // primitive) via `__torajs_any_index_get`. A null/undefined
    // receiver records a pending catchable TypeError, so the throw
    // check follows the call.
    if arr_ty == Type::Any {
        // L3b #13 (chunk 528) — string keys probe properties per ES
        // ToPropertyKey. A compile-time literal rides the full
        // member-read path (`o["k"]` ≡ `o.k`: class IC / length /
        // regexp props / probe fallback); a dynamic string key
        // probes by its runtime Str cell (a dynamic key that names
        // "length" lands the own-property probe — recorded
        // boundary).
        if let crate::ast::Expr::String(lit) = ctx.ast.get_expr(index) {
            let lit = lit.clone();
            return crate::ssa_lower_any_member::lower_any_member_read(ctx, arr_val, &lit);
        }
        if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::String)) {
            return lower_any_index_str_key(ctx, arr_val, index);
        }
        let idx_val = ctx.lower_index_operand(index);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_index_get, vec![arr_val, idx_val]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        other => panic!("ssa-lower: index access on non-array type {other:?}"),
    };
    let idx_val = ctx.lower_index_operand(index);
    if elem_ty == Type::Any {
        return lower_array_any_index(ctx, arr_val, idx_val);
    }
    let offset = ctx.emit_arr_slot_byte_offset(arr_val.clone(), idx_val, 3, is_non_deque);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(elem_ty, arr_val, offset),
        elem_ty,
        None,
    );
    Operand::Value(v)
}

fn lower_string_index(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    arr_ty: Type,
    index: ExprId,
) -> Operand {
    let idx_raw = ctx.lower_expr(index);
    let idx_val = ctx.coerce_to_i64(idx_raw);
    let cur_block = ctx.cur_block;
    let v = if arr_ty == Type::Str {
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.str_char_at, vec![arr_val, idx_val]),
            Type::Substr,
            None,
        )
    } else {
        let end = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        let cur_block = ctx.cur_block;
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_slice,
                vec![arr_val, idx_val, Operand::Value(end)],
            ),
            Type::Substr,
            None,
        )
    };
    Operand::Value(v)
}

fn lower_array_any_index(ctx: &mut LowerCtx<'_>, arr_val: Operand, idx_val: Operand) -> Operand {
    let cur_block = ctx.cur_block;
    let tag = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_get_any_tag,
            vec![arr_val.clone(), idx_val.clone()],
        ),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let value = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_get_any_value, vec![arr_val, idx_val]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let box_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(value)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(box_v)
}

/// Dynamic string key on an `any` receiver — probe by the runtime
/// Str cell (borrow); a Substr view materializes to an owned temp
/// released after the probe.
fn lower_any_index_str_key(ctx: &mut LowerCtx<'_>, obj_val: Operand, index: ExprId) -> Operand {
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    ctx.consume_if_ident(index);
    let owned = k_ty == Type::Substr;
    let key_op = ctx.coerce_to_str(k_raw, k_ty);
    let Operand::Value(key_v) = key_op else {
        panic!("ssa-lower: string index key lowered to a non-value operand");
    };
    let out = crate::ssa_lower_any_member::emit_any_member_probe(ctx, &obj_val, key_v);
    if owned {
        ctx.emit_drop_value(key_op, Type::Str);
    }
    out
}
