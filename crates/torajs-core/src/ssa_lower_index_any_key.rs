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

/// Typed array receiver with a KEY that no static lane can answer —
/// an `any` key (`a[i]`, whose runtime tag decides between an element
/// read, a property read, and a miss) or a string key (§7.1.19 makes
/// `a["length"]` ≡ `a.length` and `a["0"]` an element read, and only
/// the key's spelling tells them apart). Box both sides at the lane
/// boundary and hand them to the keyed kernel's ToPropertyKey
/// dispatch, mirroring the struct-receiver arm.
///
/// The kernel reads raw typed slots through the header's element-kind
/// field, which every array reaching here carries: a heap array
/// records it inside `box_to_any`, and a non-escaping array literal
/// is born with it baked in (`ssa_lower_array::alloc_stack_arr` —
/// such a block is FLAG_STATIC_LITERAL, which the runtime marker
/// refuses to write).
pub(crate) fn lower_typed_arr_any_key(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    arr_val: Operand,
    index: ExprId,
) -> Operand {
    let boxed = ctx.box_to_any(arr_val);
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    // A string key arrives as a Str slot; the kernel takes an
    // AnyValue. The box helpers are pure encodings, so the raw
    // operand keeps its own stake and its own drop below.
    let k_boxed = if k_ty == Type::Any {
        k_raw.clone()
    } else {
        ctx.box_to_any(k_raw.clone())
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_index_get_keyed, vec![boxed, k_boxed]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    // The kernel answers cells with an explicit +1, so the consumer
    // releases; the probe borrows the key, so only an owned temp key
    // is released here (the numeric lane's convention).
    ctx.owned_member_reads.insert(eid);
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    Operand::Value(v)
}

/// String / Substr receiver. A NUMBER key takes the character lane:
/// routed through the index-get runtime pair (RFC 20260707 residual),
/// where — unlike the charAt / slice method family, which answers the
/// empty string on OOB per §22.1.3.2 — `s[i]` OOB answers JS
/// `undefined` per §10.4.3 [[Get]], the immortal Substr-shaped
/// sentinel whose view walks to "undefined" (ToString consumers stay
/// branch-free) while identity consumers (strict-eq / typeof /
/// nullish probes) compare its address.
///
/// Any other key can't pick that lane: its spelling — or, for an
/// `any` key, its runtime tag — decides between the character,
/// `length`, a reified method and a miss. Box both sides and let the
/// keyed kernel's §7.1.19 ToPropertyKey dispatch answer, exactly as
/// the Arr receiver does. Pre-fix those keys rejected the whole
/// program ("index must be number, got String").
///
/// Lives here rather than in `ssa_lower_index` because that file's
/// `lower_from_value` sits against the 200-line rule and the file
/// itself against the 500-line one.
pub(crate) fn lower_str_recv_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    arr_val: Operand,
    arr_ty: Type,
    index: ExprId,
) -> Operand {
    if matches!(
        ctx.expr_types.get(&index),
        Some(crate::check::Type::Any | crate::check::Type::String | crate::check::Type::Symbol)
    ) {
        return lower_typed_arr_any_key(ctx, eid, arr_val, index);
    }
    let idx_raw = ctx.lower_expr(index);
    let idx_val = ctx.coerce_to_i64(idx_raw);
    let fid = if arr_ty == Type::Str {
        ctx.intrinsics.str_index_view
    } else {
        ctx.intrinsics.substr_index_view
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arr_val, idx_val]),
        Type::Substr,
        None,
    );
    Operand::Value(v)
}

/// Lower a property-key expression for the keyed kernels' §7.1.19
/// ToPropertyKey dispatch. An `any` key is already a box; an
/// UNDEFINED-typed key (the t262 dstr `{}[thrower()]` shape — a
/// throwing callee types its result Undefined) lowers as a Ptr and
/// boxes to ANY_UNDEF here, which the kernel coerces to the fixed
/// string key "undefined".
pub(crate) fn lower_any_key(ctx: &mut LowerCtx<'_>, index: ExprId) -> Operand {
    let raw = ctx.lower_expr(index);
    if matches!(
        ctx.expr_types.get(&index),
        Some(crate::check::Type::Undefined)
    ) {
        return ctx.box_to_any_from_expr(index, raw);
    }
    raw
}

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
