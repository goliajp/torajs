//! `Object.assign(target, ...sources)` namespace static method pulled
//! out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch
//! as chunk-20 of the `Expr::Call` god-arm decomp.
//!
//! S127-3 — N sources per ES §20.1.2.1. Copy each source's own
//! enumerable properties into the target, left-to-right; last-source-
//! wins emerges naturally from the per-write drop-old dance.
//!
//! RFC 20260714-objlit-accessor blade 8 — the copy walks PROPERTIES,
//! not layout slots. §20.1.2.1 step 4.c.ii reads each source key with
//! [[Get]] and writes the target with [[Set]], so an accessor on either
//! side is reached through its own half:
//!
//! * a source getter contributes what it ANSWERS (pre-fix the whole
//!   shape was rejected — the source's `__getter_v` slot made its
//!   struct type differ from the target's plain `v`);
//! * a target setter RECEIVES the value (a plain slot copy would have
//!   overwritten the setter closure);
//! * a target get-only property throws `TypeError` (ES §10.1.9) — tr
//!   used to copy the SOURCE's getter closure over it and answer the
//!   source's value, silently.
//!
//! Ownership per write, delegated to `ssa_lower_struct_own_props`:
//! a data slot takes the value's reference (a borrowed field load is
//! retained first, an owned getter result stored as-is); a setter is a
//! call, so an owned value is released after it returns.
//!
//! §20.1.2.1 is a SHALLOW copy — `Get` hands over the source's own
//! reference. tr used to deep-clone an `Arr` field (`arr_slice` +
//! per-element rc_inc), so `Object.assign(t, s); s.arr.push(x)` left
//! `t.arr` short one element where bun shares the array.
//!
//! Returns `Some(target_op)` (target passed through for chaining, per
//! spec). `None` lets the caller fall through when the callee isn't
//! `Object.assign` or `args.is_empty()`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_struct_own_props::{
    emit_prop_store, emit_prop_value, own_prop_sinks, own_props,
};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "assign" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let target_op = ctx.lower_expr(args[0]);
    let target_ty = ctx.operand_ty(&target_op);
    let Type::Obj(t_sid) = target_ty else {
        panic!("ssa-lower: Object.assign target must be a struct, got {target_ty:?}");
    };
    let t_layout = ctx.struct_layouts[t_sid.0 as usize].clone();
    let sinks = own_prop_sinks(&t_layout);
    for src_eid in &args[1..] {
        let source_op = ctx.lower_expr(*src_eid);
        let src_ty = ctx.operand_ty(&source_op);
        let Type::Obj(s_sid) = src_ty else {
            panic!("ssa-lower: Object.assign source must be a struct, got {src_ty:?}");
        };
        let s_layout = ctx.struct_layouts[s_sid.0 as usize].clone();
        for prop in own_props(&s_layout, ctx.fn_sigs) {
            // The checker matched the two property sets key-for-key, so
            // a missing sink here is a compiler bug, not a user error.
            let sink = sinks
                .iter()
                .find(|s| s.key == prop.key)
                .unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: Object.assign source property `{}` has no target sink",
                        prop.key
                    )
                })
                .sink;
            let val_ty = prop.ty();
            let (value, owned) = emit_prop_value(ctx, &source_op, &prop);
            emit_prop_store(ctx, &target_op, &sink, value, owned, val_ty);
        }
    }
    // RFC 20260705 owned-result invariant: ES answers the target;
    // the pass-through result carries its own ref (dedicated path,
    // bypasses integrity's lower_noop).
    ctx.emit_owned_result_inc(target_op.clone(), target_ty);
    Some(target_op)
}
