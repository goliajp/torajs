//! `lower_json_stringify` extracted from [`crate::ssa_lower`]
//! (chunk 158).
//!
//! Pre-extract this method was 502 LOC on `LowerCtx`. Becomes a
//! free fn here; `LowerCtx::lower_json_stringify` becomes a thin
//! pub(crate) wrapper because the body recurses on itself + several
//! ssa-lower sites call it as a method.
//!
//! Two entries: [`lower_top`] for the `JSON.stringify(value)` call
//! itself (Str/Substr lanes route the undefined sentinel to the
//! undefined VALUE via `__torajs_json_quote_str_top`), [`lower`] for
//! the composite element/field recursion (undefined → `null`).
//!
//! [`lower`] dispatches on `ty`:
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

mod composite;
mod composite_obj;

use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Top-level `JSON.stringify(value)` entry (the `Expr::Call` arm).
/// Differs from the recursive [`lower`] only on the Str / Substr
/// lanes: the undefined sentinel answers the undefined VALUE per ES
/// §25.5.1 step 12 (`__torajs_json_quote_str_top`), while inside an
/// array/object undefined stringifies to `null` (§25.5.2.4) — the
/// composite element recursion keeps going through [`lower`].
pub(crate) fn lower_top(ctx: &mut LowerCtx, val_op: Operand, ty: Type) -> Operand {
    match ty {
        Type::Str => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.json_quote_str_top, vec![val_op]),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        Type::Substr => {
            // substr_to_owned materializes the Substr sentinel as the
            // Str sentinel (identity propagates), so the _top probe
            // sees it; drop of the sentinel intermediate is a no-op.
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![val_op]),
                Type::Str,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.json_quote_str_top,
                    vec![Operand::Value(owned)],
                ),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(Operand::Value(owned), Type::Str);
            Operand::Value(v)
        }
        _ => lower(ctx, val_op, ty),
    }
}

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
        Type::Arr(arr_id) => composite::lower_arr(ctx, val_op, arr_id),
        Type::Obj(sid) => composite_obj::lower_obj(ctx, val_op, sid),
        Type::Ptr => {
            let p = ctx.intern_string_literal("null");
            Operand::Value(p)
        }
        // RFC 20260719-ns-static-value-reify B3b — an any-lane
        // argument has no static shape to unfold, so the walk happens
        // at runtime (same JsonBuilder output path, so a shape both
        // tiers can express serializes byte-identically). A NULL
        // answer is the §25.5.2 `undefined` result (a top-level
        // undefined / callable argument); the undefined Str sentinel
        // carries that through the Str-typed slot.
        Type::Any => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_json_stringify, vec![val_op]),
                Type::Str,
                None,
            );
            Operand::Value(v)
        }
        other => panic!("ssa-lower: JSON.stringify on type {other:?} not yet supported"),
    }
}
