//! Route/gate predicates for the closure-local call lane — "which
//! lane does this binding's call take, and is the recv gate even
//! reachable for it". Split from the parent when the 424-04
//! mismatch-binding route pushed it past the 500-line limit; the
//! parent keeps the emits.

use crate::ssa_lower::LowerCtx;

pub(super) fn global_argv_face_binding(ctx: &LowerCtx<'_>, callee_name: &str) -> bool {
    !ctx.locals.contains_key(callee_name) && ctx.ast.closure_argv_locals.contains(callee_name)
}

/// 刀 3 (RFC 20260815-fn-value-rest-spread) — the promoted flavor of
/// a rest-fn VALUE binding: closure-captured `const g = tail`
/// promotes to a global, its LetDecl never lowers in the calling fn
/// (so `variadic_locals` never saw it), and the forwarder wrap
/// recorded the name instead. Locals-miss scoping mirrors
/// [`global_argv_face_binding`].
pub(super) fn global_variadic_value_binding(ctx: &LowerCtx<'_>, callee_name: &str) -> bool {
    !ctx.locals.contains_key(callee_name) && ctx.ast.variadic_value_bindings.contains(callee_name)
}

/// 424-04 — the promoted flavor of a face-mismatched mutable
/// fn-typed binding (census doc in
/// `ast/forwarders_object_mismatch.rs`): a closure-captured such
/// binding promotes to a global, its LetDecl never lowers in the
/// calling fn, so `variadic_locals` never saw it. Locals-miss
/// scoping mirrors [`global_variadic_value_binding`].
pub(super) fn global_fnsig_mismatch_binding(ctx: &LowerCtx<'_>, callee_name: &str) -> bool {
    !ctx.locals.contains_key(callee_name)
        && (ctx.ast.fnsig_mismatch_bindings.contains(callee_name)
            || ctx.ast.face_mismatch_globals.contains(callee_name))
}

/// Is the runtime recv gate reachable for a callee resolved to
/// `callee_slot`? Two kills, both proofs the header flag can never
/// be set on this value:
///
/// 398-06 knife 2 — the whole-program fact: with no promoted closure
/// anywhere (`fnexpr_recv_fns` empty) the flag has no setter, so the
/// receiverless call keeps the single-path emit byte-for-byte.
/// Load-bearing beyond cost: the egraph self-tail-call rewrite
/// matches the EXACT single-call shape, and gating a self-recursive
/// named fn expression broke the match — 1M-deep recursion ran on
/// the real stack (tco-self-001, exit 139).
///
/// 403-02 — the per-binding narrowing on top: when the resolved
/// callee slot IS the enclosing named fn-expression's self slot
/// (§15.5.5 pin; slot identity is shadow-immune — a same-named param
/// / re-declared local resolves elsewhere) and the enclosing closure
/// is not promoted, the value is compile-time pinned to an ungated
/// closure. A program that ALSO has promoted closures elsewhere no
/// longer loses TCO on its self-recursion (the `ac0c7452` kill only
/// saved promoted-free programs).
pub(super) fn recv_gate_reachable(ctx: &LowerCtx<'_>, callee_slot: crate::ssa::ValueId) -> bool {
    let unpromoted_self =
        ctx.self_name_slot == Some(callee_slot) && !ctx.ast.fnexpr_recv_fns.contains(&ctx.f.name);
    !ctx.ast.fnexpr_recv_fns.is_empty() && !unpromoted_self
}

/// Does the call hand an `any` argument to an object-typed parameter
/// of `sig`? The checker admits it for a function value
/// (`check_type_of_call::any_struct_slot`), and the typed entry would
/// read the NaN-box bits as a cell pointer: the call rides the boxed
/// entry, whose adapter tests each object slot and hands a value the
/// body cannot read to the all-`any` twin.
pub(crate) fn any_into_obj_slot(
    ctx: &LowerCtx<'_>,
    args: &[crate::ast::ExprId],
    sig: crate::ssa::SigId,
) -> bool {
    let params = &ctx.fn_sigs[sig.0 as usize].0;
    args.iter().zip(params).any(|(a, p)| {
        matches!(p, crate::ssa::Type::Obj(_))
            && matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any))
    })
}
