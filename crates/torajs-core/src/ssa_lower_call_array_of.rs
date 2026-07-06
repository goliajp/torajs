//! `Array.of(...vals)` — emit the same SSA shape as a no-spread
//! array literal pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-52 of the decomp (chunks 1-51 = ...
//! + BigInt.asIntN/asUintN).
//!
//! Shape: `__torajs_arr_alloc(n)` → store len at `ARR_LEN_OFF` →
//! direct slot stores at `ARR_DATA_OFF + i*8`. Element type comes
//! from the first arg; `check.rs` already verified every arg
//! unifies on it. Empty `Array.of()` is rejected upstream (no
//! type anchor), so a non-empty `args` is guaranteed when the
//! gate matches.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `Array.of` Member-Ident shape, or empty args — though the
//! latter is upstream-rejected, we still gate defensively).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "of" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Array" || args.is_empty() {
        return None;
    }
    let n = args.len() as i64;
    // Chunk 570 — elems SHARE (raw Store below adopts the passed
    // reference): a borrow-shape arg takes +1 so the slot owns its
    // stake while the source binding keeps its own (the historical
    // consume let the array's death free the source's only ref —
    // UAF, probe-proven); owned temps keep transferring.
    let mut elem_vals: Vec<Operand> = Vec::with_capacity(args.len());
    for &aid in args {
        let v = ctx.lower_expr(aid);
        let v_ty = ctx.operand_ty(&v);
        if !ctx.expr_transfers_ownership(aid) && !v_ty.is_copy() {
            ctx.emit_rc_inc(v);
        }
        elem_vals.push(v);
    }
    let elem_ty = ctx.operand_ty(&elem_vals[0]);
    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    let cur_block = ctx.cur_block;
    let arr_ptr = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
    );
    let data = ctx.emit_arr_data_ptr(Operand::Value(arr_ptr));
    for (i, val) in elem_vals.iter().enumerate() {
        let off = (i as u64) * 8;
        ctx.f
            .append_void(cur_block, InstKind::Store(*val, data.clone(), off));
    }
    Some(Operand::Value(arr_ptr))
}
