//! The `any`-keyed index lanes, split out of `ssa_lower_index.rs`
//! when the bool-element promotion pushed that file past the
//! 500-line limit. Reading through a tagged receiver or by a
//! runtime key is a separate question from the typed-array bounds
//! branch that stayed behind; these three share the probe contract
//! (the key is borrowed, an owned temp key is released after it).
//!
//! Re-exported from `ssa_lower_index` so every existing caller path
//! is unchanged.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_array_any_index(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
) -> Operand {
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
pub(crate) fn lower_any_index_str_key(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    index: ExprId,
) -> Operand {
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    let owned = k_ty == Type::Substr;
    let key_op = ctx.coerce_to_str(k_raw.clone(), k_ty);
    let Operand::Value(key_v) = key_op else {
        panic!("ssa-lower: string index key lowered to a non-value operand");
    };
    let out = crate::ssa_lower_any_member::emit_any_member_probe(ctx, &obj_val, key_v);
    if owned {
        ctx.emit_drop_value(key_op, Type::Str);
    }
    // The probe borrows the key (dynobj/arrprops hash lookup — no rc
    // traffic, no storage), so an Ident key keeps its stake and its
    // scope drop; an owned temp key (concat result / fresh view) is
    // released here (RFC 20260705 ledger #3, 32B/iter probe).
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    out
}

/// Symbol key on an `any` receiver — §7.1.19 step 2 hands the key to
/// the lookup uncoerced, so the probe reads the Symbol cell itself and
/// the runtime side keys off its `type_tag`. Twin of
/// [`lower_any_index_str_key`] minus the coerce: there is no view /
/// wrapper form of a Symbol to materialize.
pub(crate) fn lower_any_index_symbol_key(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    index: ExprId,
) -> Operand {
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    let Operand::Value(key_v) = k_raw.clone() else {
        panic!("ssa-lower: symbol index key lowered to a non-value operand");
    };
    let out = crate::ssa_lower_any_member::emit_any_member_probe(ctx, &obj_val, key_v);
    // Borrow contract as the string-key twin: an Ident key keeps its
    // stake and its scope drop, an owned temp (a fresh `Symbol(desc)`
    // in the index position) is released after the probe reads it.
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    out
}
