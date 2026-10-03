//! The value side of a typed-array store through push / unshift /
//! fill / splice ([`crate::ssa_lower_call_arr_push`]): lower the
//! argument and fit it to the element type.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::Admits;

/// Lower args[0] and apply elem-width / type coercions shared between the
/// (a) ident-local and K.8 ident-global paths:
/// - bool → i64 coerce.
/// - i64 → f64 widen for F64-elem arrays (W4).
/// - F64 elem receiving F64 value: panic — container-width analysis missed.
/// - Substr → owned Str via `substr_to_owned` for `for (const c of "abc")
///   chars.push(c)` (for-of-str loop binds `c` as Substr 16-byte view, but
///   Arr<Str> slots are 8-byte heap-Str pointers). Returned `Operand` is
///   fresh rc=1 with no caller binding, so the share-inc is skipped.
///
/// Returns `(val_operand, val_owned_from_substr)` — the second flag tells
/// the caller to skip `emit_rc_inc` on the value (the substr_to_owned
/// already allocated a fresh ref).
pub(crate) fn coerce_push_value(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    elem_ty: Type,
) -> (Operand, bool) {
    let mut val = ctx.lower_expr(arg_eid);
    // Chunk 575 — an array value entering a container slot must be
    // self-describing for the cycle walker (chain mark; no-op for
    // non-Arr elem types). See index-assign twin.
    ctx.emit_arr_mark_kind(&val);
    val = ctx.coerce_bool_to_i64(val);
    let mut val_owned_from_substr = false;
    let cast = ctx.checked_cast(
        val,
        elem_ty,
        crate::ssa_lower_checked_cast::CastSlot::Store,
        crate::ssa_lower_checked_cast::CastScope::Inferred,
        Admits::Exact,
    );
    val = match (elem_ty, ctx.operand_ty(&val)) {
        _ if cast.is_some() => {
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            val_owned_from_substr = elem_ty == Type::Str;
            cast.expect("guard just tested it")
        }
        (Type::F64, Type::I64) => ctx.coerce_to_f64(val),
        // Any arg into a scalar/Str elem slot — unbox at the store
        // boundary (checker push/unshift 1-arg Any admit pairing).
        // The coerced Str is fresh rc=1, so it rides the same
        // skip-inc hand-off flag as the substr lane; the original
        // box's stake settles when the arg was an owned temp
        // (any-arith result), while borrow shapes stay with their
        // binding.
        (t @ (Type::F64 | Type::I64), Type::Any) => {
            let n = ctx.coerce_any_to_number(val, t);
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            n
        }
        // A read of a bool array comes back as a tagged value (it has
        // an out-of-range exit to describe), so pushing one straight
        // into another bool array arrives here. Unboxing is exact for
        // every value a bool slot can hold; a promoted `undefined`
        // lands on `false`, which is the only answer the slot has.
        (Type::Bool, Type::Any) => {
            let b = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_bool, vec![val]),
                Type::Bool,
                None,
            );
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            Operand::Value(b)
        }
        (Type::Str, Type::Any) => {
            let s = ctx.coerce_to_str(val, Type::Any);
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            val_owned_from_substr = true;
            s
        }
        (Type::I64, Type::F64) => panic!(
            "ssa-lower: f64 value into i64 array elem via push — \
             container width analysis missed this write"
        ),
        (Type::Str, Type::Substr) => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![val]),
                Type::Str,
                None,
            );
            // Chunk 743 — substr_to_owned READS the view without
            // consuming it: a fresh view mint (string index / slice
            // call) hands this lane its only ref, which nothing else
            // drops (churn: 300k `ss.push(s[i%8])` 15.96MB vs 6.3MB
            // flat — unshift twin identical). Borrow views (ident
            // reads of a live binding) stay with their owner.
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Substr);
            }
            val_owned_from_substr = true;
            Operand::Value(owned)
        }
        _ => val,
    };
    (val, val_owned_from_substr)
}
