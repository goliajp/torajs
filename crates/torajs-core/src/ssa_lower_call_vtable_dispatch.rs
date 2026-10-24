//! T-24 virtual-dispatch interception via vtable — `__dispatch_<M>`
//! synthetic call pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Call` god-arm as chunk-43 of the decomp (chunks 1-42 = ...
//! + P3.struct-method-dispatch).
//!
//! Desugar rewrites `obj.M()` (for chain methods) into a call to the
//! synthetic `__dispatch_<M>(obj, args)`. This arm bypasses that
//! stub: load the receiver's vtable_ptr at `OBJ_VTABLE_OFF`, load the
//! slot at `method_index[M] * 8`, add the vtable base back (slots are
//! `fn - table` relative offsets since r506 — the LLVM / Swift
//! relative-vtables shape, which is what keeps the table fixup-free
//! and in `__TEXT`), and `CallIndirect` through it. O(1) regardless
//! of inheritance depth — replaces the prior O(chain depth)
//! tag-switch cascade.
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
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_VTABLE_OFF};

mod devirt;

use devirt::devirt_target;
pub(crate) use devirt::overridden_below;

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
        Some(fid) => (
            Some(fid),
            ctx.fn_sig_ids.get(&fid).copied(),
            ctx.f_ret_type_hint(fid),
        ),
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
    // every body the slot resolves to is a user method
    let mut holds = crate::ssa_lower::call_hold::Holds::default();
    // the owned temps below release after the call; a throw in a later
    // argument or in the body reached drops them on the way out
    let mut arg_toks: Vec<usize> = Vec::new();
    let mut arg_ops: Vec<Operand> = Vec::with_capacity(args.len());
    for a in args {
        let v = ctx.lower_expr(*a);
        if let Some((op, ty)) = ctx.throw_temp_of(*a, &v) {
            arg_toks.push(ctx.push_throw_temp(op, ty));
        }
        holds.take(ctx, *a, v);
        arg_ops.push(v);
    }
    let owned_temps: Vec<(usize, Operand)> = args
        .iter()
        .zip(arg_ops.iter())
        .enumerate()
        .filter(|(_, (a, _))| ctx.expr_owned_shape(**a))
        .map(|(i, (_, op))| (i, *op))
        .collect();
    // §10.2.11 — an argument the call omits is `undefined`, and the
    // slot's row reads its parameter register either way. Every other
    // call lane routes through this pad; this one handed the callee a
    // short argv, so the missing register held whatever the caller
    // left there (`x.f(1)` on `f(x, y)` printed `object null` for one
    // row and a garbage any-tag for the next). owned_temps is built
    // first so its indices still address real arguments.
    crate::ssa_lower_call_terminal::pad_trailing_undef(ctx, eid, &mut arg_ops);
    // S2.42's twin, one lane over. This one also handed every argument
    // verbatim while its siblings route through the `arg_conv`
    // contract, so an i64 reaching an unannotated (`any`) parameter
    // arrived as raw bits: `class A { f(x) { return x } }` called
    // through the slot with `2` printed `null`. The signature is
    // receiver-first and so is `args` here, so both start at 1 (past
    // the slot's hidden argc, when it carries one).
    let slot_argc = slot_carries_argc(ctx, &owners[0], method_name);
    let user_at = 1 + usize::from(slot_argc);
    let coerce_owned = match sig_id {
        Some(sig) => {
            let param_tys = ctx.fn_sigs[sig.0 as usize].0.clone();
            crate::ssa_lower_call_terminal_coerce::coerce_args_by_param_tys(
                ctx,
                param_tys.get(user_at..).unwrap_or(&[]),
                args.get(1..).unwrap_or(&[]),
                arg_ops.get_mut(1..).unwrap_or(&mut []),
            )
        }
        None => Vec::new(),
    };
    if slot_argc {
        let n = crate::ssa_lower_call_terminal_headless::user_argc(ctx, eid, args.len());
        arg_ops.insert(1, Operand::ConstI64(n.saturating_sub(1) as i64));
    }
    let cur_block = ctx.cur_block;
    let r = match direct_fid {
        Some(fid) => ctx
            .f
            .append_inst(cur_block, InstKind::Call(fid, arg_ops), ret_ty, None),
        None => emit_vtable_call(ctx, method_idx, sig_id.unwrap(), ret_ty, arg_ops),
    };
    // Rotation 507 — a throwing OVERRIDE must reach the caller's
    // handler like any direct call would: the stub's `__dispatch_<M>`
    // name is never in `may_throw_fns` (its AST body only forwards to
    // the base), so the check keys on the bodies the slot can
    // resolve to — every declarer at or below the receiver's static
    // class (probe: `e.score()` on a throwing override printed 0 and
    // ran the statement after it).
    if slot_may_throw(ctx, args[0], method_name, &suffix, &owners) {
        ctx.emit_throw_check(None);
    } else {
        ctx.proofs.log_slot_call(&ctx.f.name);
    }
    for t in arg_toks {
        ctx.pop_throw_temp(t);
    }
    holds.release(ctx);
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    for (i, op) in owned_temps {
        ctx.release_owned_temp(args[i], &op);
    }
    Some(Operand::Value(r))
}

/// The slot of `method` carries the hidden argc right after the
/// receiver: its rows read `arguments.length`, and the this-first argc
/// tier admits a slot's rows all together (`declarer` is any of them).
pub(crate) fn slot_carries_argc(ctx: &LowerCtx<'_>, declarer: &str, method: &str) -> bool {
    ctx.ast
        .this_first_argc_fns
        .contains_key(&format!("__cm_{declarer}__{method}"))
}

/// True iff a body the slot may resolve to for this receiver is a
/// may-throw fn. With a `ClassRef` receiver only the declarers at or
/// below its class count; otherwise every owner does.
pub(crate) fn slot_may_throw(
    ctx: &LowerCtx<'_>,
    recv: ExprId,
    method_name: &str,
    suffix: &str,
    owners: &[String],
) -> bool {
    let static_class = match ctx.expr_types.get(&recv) {
        Some(crate::check::Type::ClassRef(key)) => {
            Some(key.split('<').next().unwrap_or(key.as_str()).to_string())
        }
        _ => None,
    };
    owners.iter().any(|o| {
        static_class.as_deref().is_none_or(|base| {
            crate::ast::method_owner_is_in_chain(&ctx.ast.class_parents, base, o)
        }) && [
            format!("__cm_{o}__{method_name}{suffix}"),
            format!("__cm_{o}__{method_name}"),
        ]
        .iter()
        .any(|n| ctx.may_throw_fns.contains(n))
    })
}

/// The slot call itself: load the receiver's vtable, load the
/// relative slot, add the table base back, `CallIndirect`. `argv[0]`
/// is the receiver. Shared with the sibling-dispatch lane, whose
/// Member-shape sites reach the same slot when the static class has
/// an overriding descendant.
pub(crate) fn emit_vtable_call(
    ctx: &mut LowerCtx<'_>,
    method_idx: u32,
    sig_id: crate::ssa::SigId,
    ret_ty: Type,
    argv: Vec<Operand>,
) -> crate::ssa::ValueId {
    let recv = argv[0];
    let cur_block = ctx.cur_block;
    let vt = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Ptr, recv, OBJ_VTABLE_OFF),
        Type::Ptr,
        None,
    );
    let rel = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, Operand::Value(vt), (method_idx as u64) * 8),
        Type::I64,
        None,
    );
    let fn_ptr = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(vt), Operand::Value(rel)),
        Type::Ptr,
        None,
    );
    ctx.f.append_inst(
        cur_block,
        InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr), argv),
        ret_ty,
        None,
    )
}
