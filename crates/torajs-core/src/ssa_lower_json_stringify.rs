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
//! - **Ptr** (S169) → `"null"`. SSA folds JS null and `undefined`
//!   into this one pointer-shaped slot, which §25.5.2.4 does not:
//!   an undefined property is omitted, a null one prints. The
//!   composite arms take that verdict from the FRONTEND type riding
//!   down beside the SSA one (rotation 208), so this arm only ever
//!   sees the null half of the pair.
//! - other → panic.

mod composite;
mod composite_obj;
mod composite_obj_jsb;
mod to_json;

use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Top-level `JSON.stringify(value)` entry (the `Expr::Call` arm).
/// Differs from the recursive [`lower`] only on the Str / Substr
/// lanes: the undefined sentinel answers the undefined VALUE per ES
/// §25.5.1 step 12 (`__torajs_json_quote_str_top`), while inside an
/// array/object undefined stringifies to `null` (§25.5.2.4) — the
/// composite element recursion keeps going through [`lower`].
pub(crate) fn lower_top(
    ctx: &mut LowerCtx,
    val_op: Operand,
    ty: Type,
    fe: Option<crate::check::Type>,
    gap: Option<Operand>,
) -> Operand {
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
        _ => lower_keyed(ctx, val_op, ty, None, fe, gap, 0),
    }
}

/// The recursive value serializer. `key` is the §25.5.2 property key
/// the value sits under — the argument a `toJSON` hook receives
/// (step 2.b). `None` is the empty key the top-level value carries;
/// composite recursion passes its property name / element index.
///
/// `fe` is the value's FRONTEND type where the walk still knows it.
/// SSA folds `undefined` and `null` into the same `Type::Ptr` slot,
/// but §25.5.2.4 keeps them apart — an undefined property is omitted
/// (step 8.b) while a null one prints `null` — so the distinction has
/// to ride down from the checker's types. `None` means the walk lost
/// track (a `toJSON` result, an unannotated shape) and the SSA type
/// decides alone, which is the pre-existing behaviour.
pub(crate) fn lower_keyed(
    ctx: &mut LowerCtx,
    val_op: Operand,
    ty: Type,
    key: Option<Operand>,
    fe: Option<crate::check::Type>,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    if let Type::Obj(sid) = ty
        && let Some(out) = to_json::try_lower_hook(ctx, &val_op, sid, key, gap.clone(), depth)
    {
        return out;
    }
    lower_shape(ctx, val_op, ty, fe, gap, depth)
}

/// The runtime walk over an any-lane value. Under a gap it takes the
/// gap entry, which also receives the nesting level so an any-typed
/// member of a statically unfolded composite keeps indenting from
/// its parent's level instead of restarting at zero.
pub(super) fn emit_any_walk(
    ctx: &mut LowerCtx,
    val_op: Operand,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    let v = match gap {
        Some(g) => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.anyv_json_stringify_gap,
                vec![val_op, g, Operand::ConstI64(depth as i64)],
            ),
            Type::Str,
            None,
        ),
        None => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_json_stringify, vec![val_op]),
            Type::Str,
            None,
        ),
    };
    Operand::Value(v)
}

/// Peel the `Nullable` wrapper the checker puts around pointer-shaped
/// types — the JSON walk cares about the payload's shape, and the
/// null case is settled by the composite arms' own null gate.
fn fe_peel(fe: Option<crate::check::Type>) -> Option<crate::check::Type> {
    match fe {
        Some(crate::check::Type::Nullable(inner)) => Some(*inner),
        other => other,
    }
}

/// Whether the checker's type names an Error-derived class instance.
/// Those receivers may not take the compile-time field unfold: two of
/// the injected layout's slots (`message` / `stack`) are `E:false`
/// per §20.5.6.1.1 and `message` may not be own at all, none of which
/// a field-name list can express. The runtime walk reads the live
/// attributes, so the Obj arm hands them over to it.
fn fe_is_error_instance(ctx: &LowerCtx, fe: Option<&crate::check::Type>) -> bool {
    match fe {
        Some(crate::check::Type::ClassRef(n)) => ctx.class_is_error_derived(n),
        _ => false,
    }
}

fn lower_shape(
    ctx: &mut LowerCtx,
    val_op: Operand,
    ty: Type,
    fe: Option<crate::check::Type>,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
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
        Type::F64 => lower_f64(ctx, val_op),
        Type::Bool => lower_bool(ctx, val_op),
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
        // Rotation 185 — Date stringifies through its toJSON
        // (§25.5.2.4 step 2): valid → quoted ISO string, invalid →
        // Str-slot NULL (JS null), which json_quote_str's nullish
        // arm turns into the "null" text. Same answer at top level
        // and inside composites, so one arm covers all three shapes
        // (direct call / struct field / typed arr element).
        Type::Date => {
            let iso = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.date_to_json, vec![val_op]),
                Type::Str,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.json_quote_str, vec![Operand::Value(iso)]),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(Operand::Value(iso), Type::Str);
            Operand::Value(v)
        }
        Type::Arr(arr_id) => {
            let fe_elem = match fe_peel(fe) {
                Some(crate::check::Type::Array(e)) => Some(*e),
                _ => None,
            };
            composite::lower_arr(ctx, val_op, arr_id, fe_elem, gap, depth)
        }
        Type::Obj(sid) => {
            let peeled = fe_peel(fe);
            let is_error = fe_is_error_instance(ctx, peeled.as_ref());
            let fe_fields = match peeled {
                Some(crate::check::Type::Struct(fs)) => Some(fs),
                _ => None,
            };
            composite_obj::lower_obj(ctx, val_op, sid, fe_fields, gap, depth, is_error)
        }
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
            let v = emit_any_walk(ctx, val_op, gap, depth);
            // The runtime walk can leave a pending throw (an accessor
            // entry's getter, the depth-cap cycle TypeError) — route
            // it into the caller's catch machinery like every other
            // throwing kernel call.
            ctx.emit_throw_check(None);
            v
        }
        // §25.5.2.4 — these classes carry no own enumerable
        // properties (their contents live in internal slots), so the
        // ordinary-object walk answers `{}`. The any-lane walk says
        // the same through its catch-all; this arm is what lets a
        // statically typed receiver agree instead of rejecting. A
        // NULL slot is JS null, same gate the Obj / Arr arms use.
        Type::Map
        | Type::Set
        | Type::WeakMap
        | Type::WeakSet
        | Type::WeakRef
        | Type::RegExp
        | Type::Promise
        | Type::MapIter
        | Type::ArrIter => composite::with_null_gate(ctx, &val_op, "__json_exotic_out", |ctx| {
            Operand::Value(ctx.intern_string_literal("{}"))
        }),
        // §25.5.2.4 step 10 (BigInt is a TypeError) and the Symbol
        // leg of the undefined/callable split (nothing at all) are
        // both value-level verdicts with no shape to unfold, so they
        // box into the any-lane walk that already implements them
        // rather than growing a second copy here. Same posture as the
        // `Type::Any` arm above, throw check included — that is what
        // turns the BigInt TypeError into the caller's catch.
        Type::BigInt | Type::Symbol => {
            let boxed = ctx.box_to_any(val_op);
            let v = emit_any_walk(ctx, boxed, gap, depth);
            ctx.emit_throw_check(None);
            v
        }
        other => panic!("ssa-lower: JSON.stringify on type {other:?} not yet supported"),
    }
}

/// ES §25.5.2.1 SerializeJSONNumber — `!IsFinite(x)` is not valid
/// JSON, so NaN / ±Infinity write `"null"`. Arm body verbatim from
/// [`lower_shape`]'s match.
fn lower_f64(ctx: &mut LowerCtx, val_op: Operand) -> Operand {
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

/// `true` / `false` — arm body verbatim from [`lower_shape`]'s match.
fn lower_bool(ctx: &mut LowerCtx, val_op: Operand) -> Operand {
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
