//! General fn-call typing tail of
//! [`crate::check_type_of_call::check`] — runs after every
//! early-route segment declines. Body verbatim from the
//! pre-split cascade: Type::Function destructure, p1_thisarg /
//! t28_pad, the arity-shaping wedge chain (now
//! [`super::arity_wedges`]), arity gate, per-arg subtype loop +
//! consume bitmap.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

mod arg_admitted;
mod args;
mod defaulted_param;

pub(crate) fn general_call(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    let callee_ty = checker.type_of(ast, *callee)?;
    // 567-05 — a callee that MAY be undefined is still called
    // through its signature. `let h: Fn | undefined; h = f; h(3)`
    // is a program bun runs, and §13.3.6.2 makes the undefined case
    // a runtime TypeError at step 6, not a compile-time refusal —
    // the same reason `is_uncallable_value` below types its call
    // rather than rejecting it. The difference is that this one is
    // callable on the arm that matters, so it keeps the signature:
    // arity and argument types are still checked against it.
    let callee_ty = match callee_ty {
        Type::Nullable(inner) if matches!(*inner, Type::Function(..)) => *inner,
        other => other,
    };
    let Type::Function(mut params, ret) = callee_ty else {
        // Cluster #4 (test262) — an Any-typed callee reaching the
        // general tail is a MEMBER READ the per-family tables
        // answered with their catch-all (`arr.hasOwnProperty` /
        // `fn.caller` / an expando property): every real typed
        // method name answers a concrete Function from its table,
        // and a bare Any Ident was already admitted by route_early.
        // TS semantics: `any` absorbs the call and answers `any`;
        // lowering routes it through the runtime any-method
        // dispatcher (gate mirror in `ssa_lower_any_method_call` —
        // both key on this Any record for the callee expr).
        if callee_ty == Type::Any {
            for a in args {
                checker.type_of(ast, *a)?;
            }
            return Ok(Type::Any);
        }
        // A callee whose static type is a KNOWN uncallable value
        // (`true()` / `f.length()` / `/re/()`) — §13.3.6.2 makes
        // that a RUNTIME TypeError (step 6), reached after the
        // arguments evaluate (step 4), so it must not stop the
        // compile. Typed Any: the call's value is unreachable.
        // Lowering mirror: `ssa_lower_call_uncallable`, keyed on
        // the same `is_uncallable_value` predicate.
        if callee_ty.is_uncallable_value() {
            for a in args {
                checker.type_of(ast, *a)?;
            }
            return Ok(Type::Any);
        }
        return Err(format!("not callable: type {callee_ty:?}"));
    };
    // Chunk 806 — a void call USED AS A VALUE is `undefined` (ES
    // §14.10.1: a return-less body completes with undefined). Typing
    // the call expression Undefined instead of Void lets every
    // Undefined-aware consumer (console print / strict-eq / typeof /
    // let-init / box_to_any) answer the bun-observable value for
    // free; annotation-mismatch rejects stay loud (Undefined admits
    // exactly where undefined does). Builtin wedges that answer
    // Void directly don't route through here and keep their shape.
    let ret = if *ret == Type::Void {
        Box::new(Type::Undefined)
    } else {
        ret
    };
    // P1 wedge — Array.prototype callback methods accept
    // an optional trailing thisArg per ES spec §23.1.3.X
    // (map/filter/every/some/forEach/find/findIndex/
    // findLast/findLastIndex/reduce/reduceRight/flatMap).
    // tora's callbacks don't have `this` semantics
    // (closures don't bind a receiver), so the thisArg
    // is silently dropped — tests that don't rely on
    // `this` inside the callback now typecheck (~70+
    // cases unblocked across the broader sample). Tests
    // that DO use `this` were already blocked on the
    // missing-this substrate; the silent drop doesn't
    // make those worse.
    let mut effective_args = args.clone();
    // P1 / S270 — Array.prototype callback methods trailing
    // thisArg drop wedge extracted to
    // [`crate::check_type_of_call_p1_thisarg`] (chunk 297).
    crate::check_type_of_call_p1_thisarg::apply(
        checker,
        ast,
        callee,
        params.len(),
        &mut effective_args,
    )?;
    // RFC 20260708-variadic — rest-param callee admit: pop the
    // trailing `Type::Rest(elem)` sentinel and stretch the param
    // list to the argument count so the arity gate and per-arg
    // loop pair rest-position args against the element type. A
    // packed DIRECT call folds first (刀 1 — see the helper doc).
    crate::check_type_of_call_rest_param::fold_packed_direct_call(
        checker,
        ast,
        *callee,
        &mut params,
    );
    let variadic = crate::check_type_of_call_rest_param::apply(&mut params, &effective_args);
    // T-28 — Default param missing → undefined widen wedge
    // extracted to [`crate::check_type_of_call_t28_pad`]
    // (chunk 298).
    if let Some(r) = crate::check_type_of_call_t28_pad::try_pad(
        checker,
        ast,
        eid,
        &params,
        &effective_args,
        &ret,
    ) {
        // a padded call through a census fn slot answers what the
        // stored value returns, as an unpadded one does
        return r.map(|ret| super::fn_slot_ret::call_ret_face(checker, ast, eid, *callee, ret));
    }
    super::arity_wedges::apply(checker, ast, callee, &mut effective_args, &mut params)?;
    super::arity_wedges::drop_extra_args(checker, ast, &mut effective_args, &params)?;
    // §18 hands a missing argument undefined, and some slots have a
    // clause that throws on it — the sibling names which.
    if let Some(msg) =
        super::arity_wedges::missing_callback_throw(checker, ast, callee, &params, &effective_args)?
    {
        checker.spec_throws.insert(eid, msg);
        return Ok(*ret);
    }
    if params.len() != effective_args.len() {
        // A variadic callee has no exact arity to name: what the call
        // fell short of is its FIXED prefix, and only a slot undefined
        // does not fit (a typed one) gets this far.
        let bound = if variadic { "at least " } else { "" };
        return Err(format!(
            "expected {bound}{} argument(s), got {}",
            params.len(),
            effective_args.len()
        ));
    }
    let Some(boxed_entry) = args::check(checker, ast, eid, callee, &params, &effective_args)?
    else {
        return Ok(*ret);
    };
    if boxed_entry {
        return Ok(super::any_struct_slot::call_ret_face(checker, eid, *ret));
    }
    let ret = super::fn_slot_ret::call_ret_face(checker, ast, eid, *callee, *ret);
    Ok(super::any_widen_ret::call_ret_face(checker, ast, eid, ret))
}
