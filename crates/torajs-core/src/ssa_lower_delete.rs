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
//! Key ledger mirrors `lower_any_index_str_key`: the runtime borrows
//! the key (hash probe, no storage), so a static Member name rides
//! the interned literal cell, an Ident key keeps its stake, and an
//! owned temp key (concat result / Substr view) is released after
//! the call.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, operand: ExprId) -> Operand {
    let (recv, key_v, owned_temp) = match ctx.ast.get_expr(operand) {
        Expr::Member { obj, name } => {
            let name = name.clone();
            let obj = *obj;
            let recv = ctx.lower_expr(obj);
            let key = ctx.intern_string_literal(&name);
            (recv, key, None)
        }
        Expr::Index { obj, index } => {
            let obj = *obj;
            let index = *index;
            let recv = ctx.lower_expr(obj);
            if let Expr::String(lit) = ctx.ast.get_expr(index) {
                let lit = lit.clone();
                let key = ctx.intern_string_literal(&lit);
                (recv, key, None)
            } else {
                let k_raw = ctx.lower_expr(index);
                let k_ty = ctx.operand_ty(&k_raw);
                // §13.5.1.2 step 5 is ToPropertyKey, so §7.1.19 step 2
                // hands a Symbol key to OrdinaryDelete untouched — the
                // entry table keys off the cell's own tag.
                let key_op = if k_ty == Type::Symbol {
                    k_raw.clone()
                } else {
                    ctx.coerce_to_str(k_raw.clone(), k_ty)
                };
                let Operand::Value(key_v) = key_op else {
                    panic!("ssa-lower: delete key lowered to a non-value operand");
                };
                let coerce_owned = k_ty == Type::Substr;
                (
                    recv,
                    key_v,
                    Some((key_op, coerce_owned, k_raw, k_ty, index)),
                )
            }
        }
        other => panic!("ssa-lower: `delete` on non-property operand {other:?}"),
    };
    let cur_block = ctx.cur_block;
    let ans = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_prop_delete,
            vec![recv, Operand::Value(key_v)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    if let Some((key_op, coerce_owned, k_raw, k_ty, index)) = owned_temp {
        if coerce_owned {
            ctx.emit_drop_value(key_op, Type::Str);
        }
        if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
            ctx.emit_drop_value(k_raw, k_ty);
        }
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
