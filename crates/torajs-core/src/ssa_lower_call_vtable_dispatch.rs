//! T-24 virtual-dispatch interception via vtable — `__dispatch_<M>`
//! synthetic call pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-43 of the decomp (chunks 1-42 = ...
//! + P3.struct-method-dispatch).
//!
//! Desugar rewrites `obj.M()` (for chain methods) into a call to the
//! synthetic `__dispatch_<M>(obj, args)`. This arm bypasses that
//! stub: load the receiver's vtable_ptr at `OBJ_VTABLE_OFF`, load the
//! slot at `method_index[M] * 8`, and `CallIndirect` through it.
//! O(1) regardless of inheritance depth — replaces the prior
//! O(chain depth) tag-switch cascade.
//!
//! The return type + signature are resolved from the base owner's
//! `__cm_<base>__<M>` fn. Every override shares the signature
//! (Liskov: subclass `__cm` has the same param + return shape as the
//! base). Args SHARE (chunk 569): non-Copy idents pass as +0
//! borrows and keep their stake — the historical blanket consume
//! orphaned every fresh-binding arg (32B/iter leak, probe-proven);
//! owned-shape temps release after the call (call_terminal mirror).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not an
//! `Ident("__dispatch_<M>")`, `M` absent from `method_owners` /
//! `method_index`, or args empty).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_VTABLE_OFF};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let method_name = callee_name.strip_prefix("__dispatch_")?;
    let owners = ctx.ast.method_owners.get(method_name).cloned()?;
    let method_idx = ctx.ast.method_index.get(method_name).copied()?;
    if args.is_empty() {
        return None;
    }
    let arg_ops: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    let owned_temps: Vec<(usize, Operand)> = args
        .iter()
        .zip(arg_ops.iter())
        .enumerate()
        .filter(|(_, (a, _))| ctx.expr_owned_shape(**a))
        .map(|(i, (_, op))| (i, *op))
        .collect();
    let recv = arg_ops[0];
    let base_fn_name = format!("__cm_{}__{method_name}", owners[0]);
    let base_fid = *ctx.fn_table.get(&base_fn_name).unwrap_or_else(|| {
        panic!("ssa-lower: __dispatch interception lost base fn `{base_fn_name}`")
    });
    let ret_ty = ctx.f_ret_type_hint(base_fid);
    let sig_id = *ctx
        .fn_sig_ids
        .get(&base_fid)
        .unwrap_or_else(|| panic!("ssa-lower: __dispatch base fn `{base_fn_name}` has no SigId"));
    let cur_block = ctx.cur_block;
    let vt = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Ptr, recv, OBJ_VTABLE_OFF),
        Type::Ptr,
        None,
    );
    let fn_ptr = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Ptr, Operand::Value(vt), (method_idx as u64) * 8),
        Type::Ptr,
        None,
    );
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), arg_ops),
        ret_ty,
        None,
    );
    for (i, op) in owned_temps {
        ctx.release_owned_temp(args[i], &op);
    }
    Some(Operand::Value(r))
}
