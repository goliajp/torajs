//! Fn-indirect-call dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` as chunk-13 of the
//! `Expr::Call` god-arm decomp (chunks 1-12 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries).
//!
//! Two related arms covered here:
//! 1. **M2 Phase B Stage 4** — call through a fn-typed local
//!    (`let f = global_fn; f(args);` or fn-typed param). Load the slot,
//!    look up the signature, emit `CallIndirect`. Per-arg Type::Any
//!    boxing when target param is Type::Any (closure-lift desugar
//!    defaults unannotated closure params to Type::Any).
//! 2. **Generalized indirect** — callee is itself a `Call` expr
//!    (`f(0)(5)` chained call), an immediately-invoked closure
//!    literal (`(() => v)()` — S158 IIFE), or a comma expression
//!    (`(0, f)()`). Lower callee, dispatch by
//!    SSA type:
//!    - `Type::Closure(sig)` — env-first ABI: load fn_ptr from
//!      `CLOSURE_FN_ADDR_OFF`, prepend env_ptr to argv, intern
//!      env-first sig.
//!    - `Type::FnSig(sig)` — direct CallIndirect with the fnsig
//!      pointer.
//!    - Non-callable types fall through to the resolve_callee panic
//!      with a clearer error message (returns `None`).
//!
//! Returns `Some(result)` when callee matches one of the two indirect
//! shapes and dispatches successfully; `None` falls through to the
//! caller's resolve_callee / M3 generic retarget path (Member callees,
//! direct Ident callees, non-callable Call/Closure callee).

use crate::ast::{Expr, ExprId};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_closure_emit::{ClosureThis, emit_closure_callee_with_this};

mod fnsig;
mod local_fnsig;
pub(crate) use fnsig::emit_fnsig_callee;
use local_fnsig::try_lower_local_fnsig;

/// Try to dispatch a fn-indirect call. Returns `Some` when one of the
/// two indirect arms matched; `None` lets the caller continue to the
/// M3 retarget / direct resolve path.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(op) = try_lower_local_fnsig(ctx, eid, callee, args) {
        return Some(op);
    }
    try_lower_call_or_closure_callee(ctx, eid, callee, args)
}

/// Generalized indirect: callee is a `Call` expr (`f(0)(5)` chained),
/// a `Closure` literal (`(() => v)()` IIFE), or an `Index` read
/// (`ops[i](x)` — chunk 733, fn-typed array elements are Closure-repr
/// so the loaded element dispatches through the env-first ABI; the
/// element read is a +0 borrow, the array binding keeps the value
/// alive across the call). Lower the callee value, dispatch by SSA
/// type. Mirrors `call_fn_value` but with explicit Void return
/// handling.
fn try_lower_call_or_closure_callee(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    // A comma expression is no Reference (§13.3.6.1): its value is the
    // last operand's, called with an undefined `this`.
    if !matches!(
        ctx.ast.get_expr(callee),
        Expr::Call { .. } | Expr::Closure { .. } | Expr::Index { .. } | Expr::Sequence { .. }
    ) && !is_choice(ctx, callee)
        && nonnull_inner(ctx, callee).is_none()
    {
        return None;
    }
    // Rotation 591 — §13.3.6.2: an Index callee is a property
    // Reference, so the call's `this` is its BASE. Lower the base
    // first: that is spec order, and it means an assignment hiding
    // among the arguments (`ops[0](ops = other)`) cannot re-point the
    // binding under the seed.
    //
    // Rotation 592 — the base is no longer restricted to an Ident.
    // 591 could only admit a shape it was safe to read TWICE, since
    // the element read below re-lowers the base itself; a nested
    // `b[0][0]()` was therefore left seeding nothing and its body
    // read `undefined` for `this` (silently — the call ran). Park the
    // lowered base under `redispatch_lowered` instead: the element
    // read consumes it in place of re-emitting, so the base
    // evaluates exactly once and any shape — a nested index, a call,
    // a chain — is admissible. The element read is a +0 borrow and
    // the base outlives it (module doc above), so the seed, which
    // boxes only inside the gate's taken arm, needs no rc account.
    //
    // Both extra conditions keep this off programs it cannot help:
    // with no promoted fn-expr anywhere the gate it feeds is compiled
    // out, and a callee the checker did not call a Function does not
    // reach the Closure arm below.
    // `hs[i]!(x)` is still a property Reference: the assertion has no
    // runtime face
    let base_id = match ctx
        .ast
        .get_expr(nonnull_inner(ctx, callee).unwrap_or(callee))
    {
        Expr::Index { obj, .. } => Some(*obj),
        _ => None,
    };
    // an element of the census (`Ast::face_slots`) is called with the
    // array as its `this` (§13.3.6.1), which a stored method reads
    let face_elem = base_id.is_some_and(|obj| elem_face_slot(ctx, obj));
    let recv_base = match base_id {
        Some(obj)
            if (face_elem || !ctx.ast.fnexpr_recv_fns.is_empty())
                && matches!(
                    ctx.expr_types.get(&callee),
                    Some(crate::check::Type::Function(..))
                ) =>
        {
            let op = ctx.lower_expr(obj);
            ctx.redispatch_lowered = Some((obj, op));
            Some(op)
        }
        _ => None,
    };
    let callee_op = ctx.lower_expr(callee);
    // A callee path that reached its value without re-reading the
    // base (the struct-like literal-key arm hands off to the member
    // reader) leaves the park standing. Drop it, or an unrelated
    // later mention of the same expression would consume a stale
    // operand.
    if let Some((parked, _)) = ctx.redispatch_lowered
        && Some(parked) == base_id
    {
        ctx.redispatch_lowered = None;
    }
    let callee_ty = ctx.operand_ty(&callee_op);
    match callee_ty {
        Type::Closure(user_sig_id) => {
            // a closure literal minted for this call (`(() => v)()`) is
            // a temp nobody else holds: parked across the call's throw
            // check, released once the call is over. An async or
            // generator literal (the async desugar's generator wrapper
            // sits on the arrow's own ExprId and is called at once)
            // answers a machine that keeps reading the env without a
            // share of its own, so that cell stays as it was. A call
            // callee (`mk(i)(1)`) is the callee's return, which every
            // return path hands over +1, borrowed arms included.
            let minted = match ctx.ast.get_expr(callee) {
                Expr::Closure { fn_name, .. } => {
                    !ctx.ast.fn_async_value_fns.contains(fn_name)
                        && !ctx.ast.genexpr_names.contains_key(fn_name)
                        && !ctx.ast.async_fn_value_exprs.contains(&callee)
                        && !ctx.ast.async_sm_value_exprs.contains(&callee)
                        && !ctx.ast.gen_fn_exprs.contains_key(&callee)
                }
                Expr::Call { .. } => ctx.expr_owned_shape(callee),
                // a choice answers a fresh closure when an arm mints one,
                // an assertion when its operand does (`hs.pop()!(x)`)
                _ => {
                    (is_choice(ctx, callee) || nonnull_inner(ctx, callee).is_some())
                        && ctx.expr_owned_shape(callee)
                }
            };
            let parked = minted.then(|| ctx.push_throw_temp(callee_op, Type::Closure(user_sig_id)));
            let result =
                emit_closure_arm(ctx, eid, callee, callee_op, user_sig_id, args, recv_base);
            if let Some(t) = parked {
                ctx.pop_throw_temp(t);
                ctx.release_owned_temp(callee, &callee_op);
            }
            Some(result)
        }
        Type::FnSig(sig_id) => Some(emit_fnsig_callee(ctx, eid, callee_op, sig_id, args)),
        // a method read off a typed receiver comes out boxed, as does an
        // element of an array whose element type admits `undefined`
        Type::Any
            if matches!(ctx.ast.get_expr(callee), Expr::Sequence { .. })
                || is_choice(ctx, callee)
                || nonnull_inner(ctx, callee).is_some() =>
        {
            Some(crate::ssa_lower_any_call::emit(
                ctx, eid, callee, callee_op, args,
            ))
        }
        _ => {
            // Non-callable — fall through to resolve_callee for the panic
            // with a clearer error message.
            None
        }
    }
}

/// Is `obj` an array whose function elements some store handed a
/// value of another face (`Ast::face_slots`)?
fn elem_face_slot(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    ctx.expr_types
        .get(&obj)
        .is_some_and(|t| crate::check::face_slots::elem_hit(&ctx.ast.face_slots, t))
}

/// The operand of a non-null assertion callee (`hs.at(i)!(x)`).
fn nonnull_inner(ctx: &LowerCtx<'_>, callee: ExprId) -> Option<ExprId> {
    match ctx.ast.get_expr(callee) {
        Expr::As { expr, ty_ann } if ty_ann == "__nonnull__" => Some(*expr),
        _ => None,
    }
}

/// A choice among values (`c ? f : g`, `f || g`, `f ?? g`): no
/// Reference, so the chosen function is called with an undefined
/// `this`, as a comma expression's is.
fn is_choice(ctx: &LowerCtx<'_>, callee: ExprId) -> bool {
    matches!(
        ctx.ast.get_expr(callee),
        Expr::Ternary { .. }
            | Expr::Nullish { .. }
            | Expr::BinOp {
                op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
                ..
            }
    )
}

/// The `Type::Closure` callee arm: env-first CallIndirect, or the
/// boxed dual entry for a rest-tail callee value.
fn emit_closure_arm(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    callee_op: Operand,
    user_sig_id: crate::ssa::SigId,
    args: &[ExprId],
    recv_base: Option<Operand>,
) -> Operand {
    // Rotation 362 — a rest-tail callee VALUE (`box[0](…)` where
    // the element's checker type is `(...args: any[]) => R`)
    // never static-dispatches: the SSA sig only carries the
    // fixed prefix, so the env-first CallIndirect argv would
    // mismatch the closure's real native arity (the argv-face
    // injected params read garbage — SIGSEGV). Route through
    // the boxed dual entry, mirroring the struct-field arm
    // (`callable_field_is_variadic`).
    if matches!(
        ctx.expr_types.get(&callee),
        Some(crate::check::Type::Function(ps, _))
            if matches!(ps.last(), Some(crate::check::Type::Rest(_)))
    ) {
        return crate::ssa_lower_call_closure_local::emit_variadic_boxed_call(
            ctx,
            callee_op,
            user_sig_id,
            args,
        );
    }
    // a call of a function that returns a value of another face than
    // its declared return (`Ast::face_slots`): the boxed entry, as for
    // a binding that holds it
    if let Expr::Call { callee: f, .. } = ctx.ast.get_expr(callee)
        && matches!(ctx.ast.get_expr(*f), Expr::Ident(g) if ctx.ast.face_slots.rets.contains(g))
    {
        let passed = &args[..crate::ast::source_argc(ctx.ast, eid, args)];
        return crate::ssa_lower_call_closure_local::emit_face_slot_call(
            ctx,
            (eid, user_sig_id),
            callee_op,
            None,
            passed,
        );
    }
    // a call through an element of the census (`hs[i]()`, `hs.at(i)()`):
    // the boxed entry, as for a property slot
    if crate::check::face_slots::elem_source(ctx.ast, callee)
        .is_some_and(|o| elem_face_slot(ctx, o))
    {
        crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, callee, &callee_op);
        let this = recv_base.map(|b| ctx.box_to_any(b));
        let passed = &args[..crate::ast::source_argc(ctx.ast, eid, args)];
        return crate::ssa_lower_call_closure_local::emit_face_slot_call(
            ctx,
            (eid, user_sig_id),
            callee_op,
            this,
            passed,
        );
    }
    // Rotation 328 — an inline PROMOTED fn-expr callee (the
    // IIFE arm): its native ABI carries the `__this: any`
    // param the Closure sig sheds, so the receiverless call
    // must re-insert the slot with a boxed `undefined` — the
    // plain env-first shape would shift every user arg by
    // one. A runtime closure value arriving through a Call /
    // Index callee cannot be promoted (the AST bar admits
    // zero-alias faces only) and keeps the plain shape.
    let this = match ctx.ast.get_expr(callee) {
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name) => {
            ClosureThis::Undef
        }
        _ => match recv_base {
            Some(op) => ClosureThis::Base(op),
            None => ClosureThis::None,
        },
    };
    // 403-03 — an fn-typed boundary can hand back the
    // undefined sentinel (coerce_to_ret's Any→Closure arm);
    // calling it must be a catchable TypeError, not a jump
    // through bytes past the sentinel header. No-op unless
    // the callee source can spell the sentinel.
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, callee, &callee_op);
    emit_closure_callee_with_this(ctx, eid, callee_op, user_sig_id, args, this)
}
