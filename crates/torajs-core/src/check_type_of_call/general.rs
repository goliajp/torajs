//! General fn-call typing tail of
//! [`crate::check_type_of_call::check`] — runs after every
//! early-route segment declines. Body verbatim from the
//! pre-split cascade: Type::Function destructure, p1_thisarg /
//! t28_pad / date_setter_narrow / splice narrows / at narrow /
//! search_0arg / math_date_trailing wedges, arity gate, per-arg
//! subtype loop + consume bitmap.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn general_call(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    let callee_ty = checker.type_of(ast, *callee)?;
    let Type::Function(mut params, ret) = callee_ty else {
        return Err(format!("not callable: type {callee_ty:?}"));
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
    // RFC 20260708-closure-argc-abi chunk 1 — length-only real-argc
    // closure binding wedge: pop the synthetic argc slot, admit
    // beyond-arity calls (extra args typecheck then drop out of the
    // pairing). Runs before T-28 so a fewer-than-declared call still
    // records its pad count against the popped param list.
    crate::check_type_of_call_closure_argc::apply(
        checker,
        ast,
        callee,
        &mut params,
        &mut effective_args,
    )?;
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
        return r;
    }
    // Date per-field setter arity narrow wedge extracted to
    // [`crate::check_type_of_call_date_setter_narrow`]
    // (chunk 299).
    crate::check_type_of_call_date_setter_narrow::apply(
        checker,
        ast,
        callee,
        &effective_args,
        &mut params,
    )?;
    // `arr.splice` / `arr.toSpliced` arity narrow wedge
    // extracted to
    // [`crate::check_type_of_call_array_splice_narrow`]
    // (chunk 300).
    crate::check_type_of_call_array_splice_narrow::apply(
        checker,
        ast,
        callee,
        &effective_args,
        &mut params,
    )?;
    // S237 splice/toSpliced 2-arg-undef arity narrow wedge
    // extracted to
    // [`crate::check_type_of_call_array_splice_2arg_undef`]
    // (chunk 301).
    crate::check_type_of_call_array_splice_2arg_undef::apply(
        checker,
        ast,
        callee,
        &mut effective_args,
        &mut params,
    )?;
    // `arr.at` / `s.at` 0-arg arity narrow wedge extracted
    // to [`crate::check_type_of_call_array_at_narrow`]
    // (chunk 302).
    crate::check_type_of_call_array_at_narrow::apply(
        checker,
        ast,
        callee,
        &effective_args,
        &mut params,
    )?;
    // Array/String search-method 0-arg arity narrow wedge
    // extracted to
    // [`crate::check_type_of_call_search_0arg`]
    // (chunk 303).
    crate::check_type_of_call_search_0arg::apply(
        checker,
        ast,
        callee,
        &effective_args,
        &mut params,
    )?;
    // S243 / S250 — Math.* / Date.<static> trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_math_date_trailing_ignore`]
    // (chunk 304).
    crate::check_type_of_call_math_date_trailing_ignore::apply(
        checker,
        ast,
        callee,
        &mut effective_args,
        &params,
    )?;
    if params.len() != effective_args.len() {
        return Err(format!(
            "expected {} argument(s), got {}",
            params.len(),
            effective_args.len()
        ));
    }
    let args = &effective_args;
    // M5.1 class-method dispatch flag derived in
    // [`crate::check_type_of_call_dispatch_flags`] (chunk 306;
    // M6.1 String borrow flag pruned in chunk 311).
    let is_class_method = crate::check_type_of_call_dispatch_flags::derive(ast, callee);
    for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
        let arg_ty = checker.type_of(ast, *arg_id)?;
        // M5.2 class-method receiver subclass prefix-subtype check extracted
        // to [`crate::check_type_of_call_class_method_subtype`] (chunk 309).
        let skip_type_check = crate::check_type_of_call_class_method_subtype::skip(
            is_class_method,
            i,
            &arg_ty,
            param_ty,
        );
        // V3-18 Nullable<T> match wedge extracted to
        // [`crate::check_type_of_call_nullable_match`]
        // (chunk 308).
        let nullable_match = crate::check_type_of_call_nullable_match::matches(param_ty, &arg_ty);
        // S133 callback Function subtype carve-out extracted
        // to [`crate::check_type_of_call_callback_subtype`]
        // (chunk 307).
        let callback_subtype =
            crate::check_type_of_call_callback_subtype::matches(param_ty, &arg_ty);
        // RFC 20260707 chunk 626 — T-11 container widen at the call
        // boundary: an `Array(Any)` param admits any concrete
        // `Array(T)` arg (mirror of check_assignable's T-11 arm).
        // Every lowering call-arg station pairs this admit with
        // `emit_arr_mark_kind` per the RFC §2 protocol so the
        // callee's kind-aware Arr<Any> readers decode the raw block.
        let t11_arr_any = matches!(param_ty, Type::Array(el) if matches!(**el, Type::Any))
            && matches!(arg_ty, Type::Array(_));
        // Chunk 641 — an empty `[]` literal argument has no element
        // to infer from and types Array(Any); any Array(T) param
        // admits it contextually (`take([])`, `new N([])` — bun
        // accepts, l17b/l17e). The lowering side pairs this with a
        // param-typed empty alloc (`try_lower_empty_array_arg`) so
        // the callee never sees a FLAG_ARR_ANY block behind a typed
        // param slot.
        let empty_lit_into_arr = matches!(param_ty, Type::Array(_))
            && matches!(
                ast.get_expr(*arg_id),
                crate::ast::Expr::Array(els) if els.is_empty()
            );
        if !skip_type_check
            && !nullable_match
            && !callback_subtype
            && !t11_arr_any
            && !empty_lit_into_arr
            && param_ty != &Type::Any
            && &arg_ty != param_ty
        {
            return Err(format!(
                "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
            ));
        }
        // TS-shape: function parameters SHARE non-Copy args —
        // calling `f(x)` never marks `x` moved. The consuming-
        // params bitmap retired in chunk 568: every store lane
        // takes its own +1 (chunks 564-567), so the historical
        // caller-side consume double-counted into a leak.
    }
    Ok(*ret)
}
