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
    let suffix = ctx
        .call_retargets
        .get(&eid)
        .and_then(|r| r.strip_prefix(callee_name.as_str()))
        .unwrap_or("")
        .to_string();
    // Devirtualization fast path — the receiver's STATIC class with
    // no method-declaring descendant makes the slot invariant across
    // every possible dynamic type (closed world: `method_owners`
    // lists every declarer), so the call goes direct — no vtable
    // load, and LLVM may inline it. A declaring descendant keeps the
    // polymorphic CallIndirect below.
    let devirt = devirt_target(ctx, args[0], method_name, &suffix, &owners);
    // Both resolutions happen BEFORE lowering args — a `?` after
    // lowering would hand the fallback lane already-emitted effects.
    let (direct_fid, sig_id, ret_ty) = match devirt {
        Some(fid) => (Some(fid), None, ctx.f_ret_type_hint(fid)),
        None => {
            let base_bare = format!("__cm_{}__{method_name}", owners[0]);
            let base_fid = match ctx.fn_table.get(&base_bare) {
                Some(fid) => *fid,
                None => *ctx.fn_table.get(&format!("{base_bare}{suffix}"))?,
            };
            let sig = *ctx.fn_sig_ids.get(&base_fid)?;
            (None, Some(sig), ctx.f_ret_type_hint(base_fid))
        }
    };
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
    let r = match direct_fid {
        Some(fid) => ctx
            .f
            .append_inst(cur_block, InstKind::Call(fid, arg_ops), ret_ty, None),
        None => {
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
            ctx.f.append_inst(
                cur_block,
                InstKind::CallIndirect(sig_id.unwrap(), Operand::Value(fn_ptr), arg_ops),
                ret_ty,
                None,
            )
        }
    };
    for (i, op) in owned_temps {
        ctx.release_owned_temp(args[i], &op);
    }
    Some(Operand::Value(r))
}

/// The receiver's statically-settled slot target, or `None` to keep
/// the polymorphic vtable path. Settled means: the receiver's static
/// type is a ClassRef whose class has NO method-declaring descendant
/// (an instance can only wear that class or a descendant, and with
/// none of them re-declaring the method, every dynamic type resolves
/// the slot to the same impl). Resolution mirrors `populate_vtables`:
/// the mono spelling (the call's retarget suffix) first, then bare,
/// up the ancestor chain.
fn devirt_target(
    ctx: &LowerCtx<'_>,
    recv: ExprId,
    method_name: &str,
    suffix: &str,
    owners: &[String],
) -> Option<crate::ssa::FuncId> {
    let crate::check::Type::ClassRef(key) = ctx.expr_types.get(&recv)? else {
        return None;
    };
    let base = key.split('<').next().unwrap_or(key.as_str());
    if !ctx.ast.class_parents.contains_key(base) {
        return None;
    }
    // Any declarer below the static class keeps the vtable path.
    let overridden_below = owners.iter().any(|c| {
        if c == base {
            return false;
        }
        let mut cur = ctx.ast.class_parents.get(c).and_then(|p| p.clone());
        let mut depth = 0u32;
        while let Some(p) = cur {
            if depth > 64 {
                break;
            }
            if p == base {
                return true;
            }
            cur = ctx.ast.class_parents.get(&p).and_then(|q| q.clone());
            depth += 1;
        }
        false
    });
    if overridden_below {
        return None;
    }
    let mut cur = Some(base.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            break;
        }
        let suffixed = format!("__cm_{name}__{method_name}{suffix}");
        let bare = format!("__cm_{name}__{method_name}");
        if let Some(fid) = ctx
            .fn_table
            .get(&suffixed)
            .or_else(|| ctx.fn_table.get(&bare))
        {
            return Some(*fid);
        }
        if owners.iter().any(|o| *o == name) {
            // This level DECLARES the method yet neither spelling
            // resolves — a generic declarer whose specialization
            // this call's suffix can't name (the dispatcher's
            // retarget suffix is the BASE's, not the receiver's).
            // Falling through to an ancestor would silently drop
            // the override; the vtable row (whose resolution rides
            // the factory's own suffix) knows better.
            return None;
        }
        cur = ctx.ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    None
}
