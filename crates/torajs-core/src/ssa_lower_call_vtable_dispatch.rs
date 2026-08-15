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
//! `method_index`, args empty, or no resolvable slot signature —
//! a generic base owner whose mono the checker never retargeted
//! falls back to the plain call lanes instead of panicking).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_VTABLE_OFF};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    let callee_name = callee_name.clone();
    let method_name = callee_name.strip_prefix("__dispatch_")?;
    let owners = ctx.ast.method_owners.get(method_name).cloned()?;
    let method_idx = ctx.ast.method_index.get(method_name).copied()?;
    if args.is_empty() {
        return None;
    }
    // The base owner's fn supplies the shared slot signature (Liskov:
    // every override matches it). A GENERIC base's bare `__cm_` name
    // never enters fn_table (pass 1 skips type_params carriers) — but
    // the checker retargeted THIS call to the dispatcher's mono
    // (`__dispatch_<M>$$<suffix>`), and the same suffix names the base
    // impl's specialization, which the stub-body recheck seeded. Both
    // misses → None: the call falls through to the plain generic-fn
    // lane (the retargeted stub — static forwarding, no vtable), the
    // honest degradation over a panic.
    let base_bare = format!("__cm_{}__{method_name}", owners[0]);
    let base_fid = match ctx.fn_table.get(&base_bare) {
        Some(fid) => *fid,
        None => {
            let retarget = ctx.call_retargets.get(&eid)?;
            let suffix = retarget.strip_prefix(callee_name.as_str())?;
            *ctx.fn_table.get(&format!("{base_bare}{suffix}"))?
        }
    };
    let ret_ty = ctx.f_ret_type_hint(base_fid);
    // Resolve the sig BEFORE lowering args — a `?` after lowering
    // would hand the fallback lane already-emitted arg effects.
    let sig_id = *ctx.fn_sig_ids.get(&base_fid)?;
    let arg_ops: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    let owned_temps: Vec<(usize, Operand)> = args
        .iter()
        .zip(arg_ops.iter())
        .enumerate()
        .filter(|(_, (a, _))| ctx.expr_owned_shape(**a))
        .map(|(i, (_, op))| (i, *op))
        .collect();
    let recv = arg_ops[0];
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
