//! `Expr::Delete` lowering — `delete obj.k` / `delete obj[k]` on an
//! `any` receiver (ES §13.5.1; RFC 20260711-delete-forin-
//! propertyhelper chunk A).
//!
//! The parser only admits Member / Index operands and the checker
//! pins the receiver to `Type::Any` with a string key, so this arm
//! is a single runtime dispatch: `__torajs_any_prop_delete(recv,
//! key-Str-cell)` (dynobj OrdinaryDelete / Arr + Closure expando
//! delete / struct answers false / non-object receivers answer true
//! — per-receiver table in `torajs-anyvalue/src/prop_delete.rs`). A
//! null / undefined receiver records a pending catchable TypeError,
//! so the throw check follows the call. Result is the i64 answer
//! compared against 0 into a Bool.
//!
//! Which kernel flavour answers a REFUSED delete depends on the
//! compile goal — see [`finish`].
//!
//! Key ledger mirrors `lower_any_index_str_key`: the runtime borrows
//! the key (hash probe, no storage), so a static Member name rides
//! the interned literal cell, an Ident key keeps its stake, and an
//! owned temp key (concat result / Substr view) is released after
//! the call.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The kernel takes an AnyValue receiver. The checker admits `any`,
/// arrays, structs and class instances, and a typed binding names the
/// same heap cell — it only needs its tag put back on.
/// `box_to_any_from_expr` is rc-neutral and the kernel borrows the
/// receiver, so no stake changes hands and there is nothing to release
/// afterwards.
fn lower_receiver(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    let recv = ctx.lower_expr(obj);
    let ty = ctx.operand_ty(&recv);
    if ty == Type::Any {
        return recv;
    }
    // A number[] can hold a hole only in its F64 lane (the sentinel);
    // one the width analysis kept integer or boolean has no word for
    // it, and the kernel would refuse the write at run time.
    if let Type::Arr(id) = ty
        && matches!(
            ctx.arr_layouts[id.0 as usize],
            Type::I64 | Type::I32 | Type::Bool
        )
    {
        panic!(
            "ssa-lower: `delete` on an element of an array held in an integer or \
             boolean lane is not yet supported (the lane has no word for a hole)"
        );
    }
    ctx.box_to_any_from_expr(obj, recv)
}

/// §13.5.1.2 step 4.b — a Super Reference operand is a ReferenceError
/// whatever the property is, and whether or not it exists. The parser
/// encodes `super.x` / `super[k]` as a member off the `__superbase__`
/// marker, but the super desugars rewrite a delete operand to a plain
/// member off a MINTED base — they must, because routing it through
/// the read kernel breaks `delete <a call>` — so by here the marker is
/// gone. `Ast::super_delete_operands` carries the judgement from the
/// pass that still knew it.
///
/// Step 1 evaluates the operand first, so `delete super[(n = 1, "k")]`
/// performs the assignment and then throws. The base itself is the
/// inert marker and is never lowered.
fn try_super_property_raise(ctx: &mut LowerCtx<'_>, operand: ExprId) -> Option<Operand> {
    if !ctx.ast.super_delete_operands.contains(&operand) {
        return None;
    }
    let index = match ctx.ast.get_expr(operand) {
        Expr::Index { index, .. } => Some(*index),
        _ => None,
    };
    if let Some(k) = index {
        let _ = ctx.lower_expr(k);
    }
    let raiser = ctx.intrinsics.throw_reference_error_super_delete;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(raiser, vec![]));
    ctx.emit_throw_check(None);
    // The value lane past the throw-check is dead; `false` stands in
    // so the enclosing expression still types out, the same posture
    // the tdz raise takes.
    Some(Operand::ConstBool(false))
}

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, operand: ExprId) -> Operand {
    if let Some(op) = try_super_property_raise(ctx, operand) {
        return op;
    }
    let (recv, key_v, owned_temp) = match ctx.ast.get_expr(operand) {
        Expr::Member { obj, name } => {
            let name = name.clone();
            let obj = *obj;
            let recv = lower_receiver(ctx, obj);
            let key = ctx.intern_string_literal(&name);
            (recv, key, None)
        }
        Expr::Index { obj, index } => {
            let obj = *obj;
            let index = *index;
            let recv = lower_receiver(ctx, obj);
            if let Expr::String(lit) = ctx.ast.get_expr(index) {
                let lit = lit.clone();
                let key = ctx.intern_string_literal(&lit);
                (recv, key, None)
            } else {
                let k_raw = ctx.lower_expr(index);
                let k_ty = ctx.operand_ty(&k_raw);
                // An `any` key names its kind only at run time, and
                // ToPropertyKey decides on the value — stringifying it
                // would delete under "Symbol(x)" and leave the real
                // entry in place. Same resolver and releaser the
                // define family and `in` ask.
                if k_ty == Type::Any {
                    let k = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![k_raw]),
                        Type::Ptr,
                        None,
                    );
                    ctx.release_owned_temp(index, &k_raw);
                    ctx.emit_throw_check(None);
                    return finish(ctx, recv, k, true);
                }
                // §13.5.1.2 step 5 is ToPropertyKey, so §7.1.19 step 2
                // hands a Symbol key to OrdinaryDelete untouched — the
                // entry table keys off the cell's own tag.
                let key_op = if k_ty == Type::Symbol {
                    k_raw
                } else {
                    ctx.coerce_to_str(k_raw, k_ty)
                };
                let Operand::Value(key_v) = key_op else {
                    panic!("ssa-lower: delete key lowered to a non-value operand");
                };
                // Every coerced key is this site's own Str: the number
                // and bigint ToStrings mint one (a Substr flattens into
                // one), and the boolean face hands back an interned
                // literal whose release is a no-op. Only a Str key (the
                // identity arm) and a Symbol key (not coerced) belong to
                // someone else. Releasing just the Substr case leaked a
                // Str per `delete o[i]` (300k: 11.8 MB vs 2.1 MB).
                let coerce_owned = !matches!(k_ty, Type::Str | Type::Symbol);
                (
                    recv,
                    key_v,
                    Some((key_op, coerce_owned, k_raw, k_ty, index)),
                )
            }
        }
        other => panic!("ssa-lower: `delete` on non-property operand {other:?}"),
    };
    let b = finish(ctx, recv, key_v, false);
    if let Some((key_op, coerce_owned, k_raw, k_ty, index)) = owned_temp {
        if coerce_owned {
            ctx.emit_drop_value(key_op, Type::Str);
        }
        if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
            ctx.emit_drop_value(k_raw, k_ty);
        }
    }
    b
}

/// Call the delete kernel with an already-resolved key cell and fold
/// the i64 answer to a Bool. `release_key` releases the cell through
/// the kind-aware dropper — the resolved key may be a Str or a Symbol
/// and the two do not free the same way.
///
/// §13.5.1.2 step 5: a refused delete (a non-configurable own
/// property) throws only when the delete expression is STRICT — sloppy
/// code just answers `false`. OrdinaryDelete itself never throws for
/// it, which is why the kernel ships both flavours and the choice is
/// made here. tr's strictness granularity is the compile goal, so a
/// `"use strict"` function inside a `.cts` script answers `false`
/// where the spec would throw — a narrower divergence than throwing
/// for every sloppy program, which is what picking the throwing
/// kernel unconditionally did.
///
/// The nullish-receiver TypeError is unaffected: that one is ToObject
/// on the reference base, not the refusal, and both flavours record
/// it. Hence the throw check stays either way.
///
/// Rotation 625 — the goal is the FUNCTION's, not the program's
/// (`cur_fn_is_sloppy`): a `"use strict"` body and every class body
/// are strict inside a sloppy Script.
fn finish(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    key_v: crate::ssa::ValueId,
    release_key: bool,
) -> Operand {
    let kernel = if crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx) {
        ctx.intrinsics.any_prop_delete_soft
    } else {
        ctx.intrinsics.any_prop_delete
    };
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(kernel, vec![recv, Operand::Value(key_v)]),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    if release_key {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.anyv_property_key_drop,
                vec![Operand::Value(key_v)],
            ),
        );
    }
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(ans), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(b)
}
