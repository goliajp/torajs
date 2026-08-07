//! `Type::Any` receiver arms of the Member-assign ladder — split
//! out of `ssa_lower_assign_member.rs` (file-size line): the
//! concrete-value pack, the Any-payload unbox path, and the shared
//! tag-gated `__torajs_any_member_set` tail (literal-key shell +
//! the L3b #13 key-parameterized core the dynamic-string-key index
//! write reuses). Semantics documented on each fn — behavior is
//! byte-for-byte the pre-split ladder.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_dynobj_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    value: ExprId,
    obj_ident: &Option<String>,
    recv_owned: bool,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — SHARE: the dynobj bucket takes its own +1 (the
    // refcounted arm's rc_inc / the Any payload path's
    // any_payload_rc_inc); no consume, so a borrow-shape rhs keeps
    // the source binding's stake and an owned temp releases its
    // surplus reference after the store.
    let transfers = ctx.expr_transfers_ownership(value);
    let v_ty = ctx.operand_ty(&v_raw);
    let v_keep = v_raw.clone();
    let (tag_op, val_op): (Operand, Operand) = match v_ty {
        Type::I64 | Type::I32 => (Operand::ConstI64(2), v_raw),
        Type::F64 => {
            let cur_block = ctx.cur_block;
            let bits =
                ctx.f
                    .append_inst(cur_block, InstKind::BitCastF64ToI64(v_raw), Type::I64, None);
            (Operand::ConstI64(3), Operand::Value(bits))
        }
        Type::Bool => {
            let cur_block = ctx.cur_block;
            let zext =
                ctx.f
                    .append_inst(cur_block, InstKind::ZExtBoolToI64(v_raw), Type::I64, None);
            (Operand::ConstI64(1), Operand::Value(zext))
        }
        // Rotation 185 — a Str / Substr slot may hold a nullish repr
        // (NULL = JS null, the undefined sentinel cell / view); the
        // static tag-4 pack encoded the sentinel as a heap cell, so
        // `o.a = m[1]` (missed capture) read back as the string
        // "undefined". Same pair decode as box_to_tag_value; the
        // value half takes the bucket's +1 for a heap cell (chunk
        // 566 SHARE contract — rc/drop no-op on the sentinels).
        Type::Str => {
            let cur_block = ctx.cur_block;
            let tag_v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.anyv_str_slot_tag, vec![v_raw]),
                Type::I64,
                None,
            );
            let val_v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.anyv_str_slot_value, vec![v_raw]),
                Type::I64,
                None,
            );
            (Operand::Value(tag_v), Operand::Value(val_v))
        }
        Type::Substr => {
            let cur_block = ctx.cur_block;
            let tag_v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.anyv_substr_slot_tag, vec![v_raw]),
                Type::I64,
                None,
            );
            let val_v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.anyv_substr_slot_value, vec![v_raw]),
                Type::I64,
                None,
            );
            (Operand::Value(tag_v), Operand::Value(val_v))
        }
        // P4.0 — Type::Any must be unboxed BEFORE the is_refcounted
        // catch-all (see matching arm-order fix in
        // lower_dynobj_init). Step 7c: shim Call instead of inline
        // +8/+16 direct-offset Load (layout-decoupling).
        Type::Any => {
            let r =
                lower_dynobj_assign_any_payload(ctx, obj_val, field, v_raw, obj_ident, recv_owned);
            if transfers {
                // Owned Any temp: dropping the box releases its
                // payload stake and frees the shell; the bucket
                // keeps the payload +1 minted inside.
                ctx.emit_drop_value(v_keep, Type::Any);
            }
            return r;
        }
        _ if v_ty.is_refcounted() => {
            ctx.emit_rc_inc(v_raw);
            (Operand::ConstI64(4), v_raw)
        }
        // 2026-07-16 — an `undefined` literal RHS (`obj.k = undefined`)
        // reaches here as ConstPtrNull; the checker still tagged
        // `expr_types[&value]` as `Type::Undefined`. Pre-fix both
        // undefined and null collapsed to tag 0 (null), so
        // `obj.length = undefined` read back as `null` (test262
        // `Array/prototype/join/S15.4.4.5_A2_T1.js` #4). Mirror
        // `lower_to_tag_value_raw`'s expr-aware undef gate.
        Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => {
            if matches!(
                ctx.expr_types.get(&value),
                Some(crate::check::Type::Undefined)
            ) {
                (Operand::ConstI64(5), Operand::ConstI64(0))
            } else {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
        }
        _ => panic!("ssa-lower: dynobj assign unsupported value type {v_ty:?}"),
    };
    emit_any_member_set(ctx, obj_val, field, tag_op, val_op, obj_ident, recv_owned);
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_keep, v_ty);
    }
    Operand::ConstI64(0)
}

/// Shared tail of both `any`-receiver member-write paths — the
/// tag-gated `__torajs_any_member_set` call (RFC 20260704 C4+).
/// The receiver rides an AnyValue slot so the DynObj arm's
/// realloc-on-grow relocation writes the fresh cell back; a named
/// Ident receiver then re-stores its local from the slot (the
/// Step 7d-A write-back, now covering the Any-payload path too).
/// The hint interns the compile-time member name for the RegExp
/// `lastIndex` / Arr `length` runtime arms.
pub(crate) fn emit_any_member_set(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    tag_op: Operand,
    val_op: Operand,
    obj_ident: &Option<String>,
    recv_owned: bool,
) {
    let key_str = ctx.intern_string_literal(field);
    let hint = if field == "length" {
        torajs_rc::ANY_WPROP_ARR_LENGTH
    } else {
        torajs_rc::any_regexp_prop_id(field).unwrap_or(-1)
    };
    emit_any_member_set_dyn(
        ctx, obj_val, key_str, hint, tag_op, val_op, obj_ident, recv_owned,
    );
}

/// Key-parameterized core of [`emit_any_member_set`] — the L3b #13
/// dynamic-string-key index write reuses it with a runtime Str cell
/// key and no compile-time hint (a dynamic key that names
/// `lastIndex` / `length` misses their hint arms — recorded
/// boundary, loud not silent).
///
/// `recv_owned` — the receiver operand is a fresh owned box (an
/// inline `(expr as any)` cast, a member-read chain) rather than a
/// borrow of a binding's stake. The kernel reads the receiver
/// through the slot without consuming it, and an owned box used to
/// have NO release site here at all. That is a leak, and worse than
/// one: the stranded +1 sits on the receiver during the at-exit
/// cycle drain, so a prototype cycle that a clean run collects as
/// ONE white group gets cut in two — the first half's teardown drops
/// the second half's targets to zero early, and the second half then
/// decs through freed memory. `(TypeError.prototype as any).x = 1`
/// was a one-line reproduction: rc underflow on %Error.prototype%.
/// The drop reads the slot (not the pre-call operand) because the
/// dynobj-resize arm writes the relocated cell back into it.
pub(crate) fn emit_any_member_set_dyn(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    key_str: crate::ssa::ValueId,
    hint: i64,
    tag_op: Operand,
    val_op: Operand,
    obj_ident: &Option<String>,
    recv_owned: bool,
) {
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Store(obj_val, Operand::Value(slot), 0));
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_set,
            vec![
                Operand::Value(slot),
                Operand::Value(key_str),
                tag_op,
                val_op,
                Operand::ConstI64(hint),
            ],
        ),
    );
    // P3.attribute-flag-tracking + the tag-gate boundary TypeErrors.
    ctx.emit_throw_check(None);
    if recv_owned {
        // An owned receiver has no binding to write back to; its box
        // stake releases here instead (see the fn doc). Sits after the
        // throw check exactly as the write-back does — the throw path
        // skips both, which is the pre-existing posture of this slot.
        let cur_block = ctx.cur_block;
        let fresh = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        );
        ctx.emit_drop_value(Operand::Value(fresh), Type::Any);
        return;
    }
    let Some(name) = obj_ident else { return };
    let Some(info) = ctx.locals.get(name).copied() else {
        return;
    };
    if !matches!(info.ty, Type::Any) {
        return;
    }
    let cur_block = ctx.cur_block;
    let fresh = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
        None,
    );
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(fresh), Operand::Value(info.slot), 0),
    );
}

fn lower_dynobj_assign_any_payload(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    field: &str,
    v_raw: Operand,
    obj_ident: &Option<String>,
    recv_owned: bool,
) -> Operand {
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v_raw]),
        Type::I64,
        None,
    );
    // Chunk 610 — owned unbox fuses unbox_value + payload_rc_inc
    // (ShortStr materialize was double-counted by the separate inc
    // and leaked).
    let cur_block = ctx.cur_block;
    let val_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value_owned, vec![v_raw]),
        Type::I64,
        None,
    );
    emit_any_member_set(
        ctx,
        obj_val,
        field,
        Operand::Value(tag_v),
        Operand::Value(val_v),
        obj_ident,
        recv_owned,
    );
    v_raw
}
