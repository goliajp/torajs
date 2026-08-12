//! General fn-call typing tail of
//! [`crate::check_type_of_call::check`] — runs after every
//! early-route segment declines. Body verbatim from the
//! pre-split cascade: Type::Function destructure, p1_thisarg /
//! t28_pad / date_setter_narrow / splice narrows / at narrow /
//! search_0arg / math_date_trailing wedges, arity gate, per-arg
//! subtype loop + consume bitmap.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

use super::any_widen::{any_into_heap_param, arr_any_into_typed_arr_param, as_cast_inner_ty};

pub(crate) fn general_call(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    let callee_ty = checker.type_of(ast, *callee)?;
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
    // loop pair rest-position args against the element type.
    crate::check_type_of_call_rest_param::apply(&mut params, &effective_args)?;
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
    apply_arity_narrow_wedges(checker, ast, callee, &mut effective_args, &mut params)?;
    // RFC 20260810-indirect-argc-abi S3.5 — beyond-arity calls are
    // universally admitted (ES §13.3.6.1: every argument evaluates,
    // extras never bind a parameter). The extras still typecheck —
    // a type error inside one stays loud — then drop out of the
    // pairing; the lowering lanes evaluate them for their side
    // effects without letting them enter argv, and argc-carrying
    // ABIs pass the REAL count. Replaces the name-keyed length-only
    // closure wedge (check_type_of_call_closure_argc): the general
    // rule covers its list.
    if effective_args.len() > params.len() {
        for a in &effective_args[params.len()..] {
            checker.type_of(ast, *a)?;
        }
        effective_args.truncate(params.len());
    }
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
        if !arg_admitted(
            checker,
            ast,
            eid,
            callee,
            is_class_method,
            i,
            param_ty,
            &arg_ty,
            arg_id,
        ) {
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

/// Post-T-28 arity-shaping wedge chain — the per-method narrows
/// (Date setters / splice / at / search-0arg) plus the trailing-arg
/// ignore pair (Math-Date receivers, then the direct bare-Ident /
/// IIFE callee). All same-shape `apply(..)?` calls with no early
/// return; each wedge's contract lives in its own module doc.
fn apply_arity_narrow_wedges(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &mut Vec<ExprId>,
    params: &mut Vec<Type>,
) -> Result<(), String> {
    // Date per-field setter arity narrow wedge (chunk 299).
    crate::check_type_of_call_date_setter_narrow::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // `arr.splice` / `arr.toSpliced` arity narrow wedge (chunk 300).
    crate::check_type_of_call_array_splice_narrow::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // S237 splice/toSpliced 2-arg-undef arity narrow wedge (chunk 301).
    crate::check_type_of_call_array_splice_2arg_undef::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // `arr.at` / `s.at` 0-arg arity narrow wedge (chunk 302).
    crate::check_type_of_call_array_at_narrow::apply(checker, ast, callee, effective_args, params)?;
    // Array/String search-method 0-arg arity narrow wedge (chunk 303).
    crate::check_type_of_call_search_0arg::apply(checker, ast, callee, effective_args, params)?;
    // S243 / S250 — Math.* / Date.<static> trailing-arg ignore wedge
    // (chunk 304).
    crate::check_type_of_call_math_date_trailing_ignore::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    // Direct-call (bare-Ident / IIFE callee) trailing-arg ignore
    // wedge — ES §13.3.6.1 evaluates every arg, the callee binds
    // only its declared formals.
    crate::check_type_of_call_direct_trailing_ignore::apply(
        checker,
        ast,
        callee,
        effective_args,
        params,
    )?;
    Ok(())
}

/// Per-arg admit predicate — strict equality plus the accumulated
/// carve-out wedges (chunk 762 extraction; the wedge stack had
/// grown the loop body past the 200-line fn limit).
#[allow(clippy::too_many_arguments)]
fn arg_admitted(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    is_class_method: bool,
    i: usize,
    param_ty: &Type,
    arg_ty: &Type,
    arg_id: &ExprId,
) -> bool {
    // r381 — an `as` cast over an `any` value changes what the CHECKER
    // sees and nothing about the runtime repr, so `f(z as P)` used to
    // slip past the equality shortcut below and hand a NaN-boxed value
    // to a reader using P's declared offsets (struct fields answered
    // `0` / `null`; a fn-typed param answered SIGBUS). Route it to the
    // same monomorph lane the UNCAST spelling already takes — the two
    // spellings must not disagree, and that lane is the one whose
    // answers were verified bun-equal.
    if let Some(inner) = as_cast_inner_ty(checker, ast, arg_id)
        && (any_into_heap_param(checker, ast, eid, callee, &inner, param_ty, i)
            || arr_any_into_typed_arr_param(checker, ast, eid, callee, &inner, param_ty, i))
    {
        return true;
    }
    if param_ty == &Type::Any || arg_ty == param_ty {
        return true;
    }
    // RFC 20260715-nominal-class-identity — a class instance types as
    // its NAME, so every structural wedge below (subclass prefix,
    // Nullable match, callback subtype, container widen) needs the
    // shape behind it. Two DIFFERENT class names still compare
    // structurally, which is what TS assignability is (a TypeError is
    // assignable to an Error param because its layout is a prefix).
    let param_ty = &crate::check::resolve_class_ref(
        param_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    let arg_ty = &crate::check::resolve_class_ref(
        arg_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    if arg_ty == param_ty {
        return true;
    }
    // Rotation 365 fn-arg track — a rest-tail closure value (an
    // argv-face fn-expr's public type) admits into a declared
    // closure-typed param ONLY where the argv tier marked that exact
    // (fn, param) boxed (`ast.argv_boxed_params`): the SSA variadic
    // registration then routes the param's direct calls through the
    // boxed dual entry, which serves the reshaped signature the
    // rest-tail type advertises. The return types still pair (a Void
    // param slot discards any return).
    if let Type::Function(ps, aret) = arg_ty
        && matches!(ps.last(), Some(Type::Rest(_)))
        && let Type::Function(_, pret) = param_ty
        && (pret.as_ref() == &Type::Void || pret == aret)
        && let crate::ast::Expr::Ident(g) = ast.get_expr(*callee)
        && ast.argv_boxed_params.get(g).is_some_and(|set| {
            ast.stmts.iter().any(|s| {
                matches!(s, crate::ast::Stmt::FnDecl { name, params, .. }
                    if name == g && params.get(i).is_some_and(|p| set.contains(&p.name)))
            })
        })
    {
        return true;
    }
    // M5.2 class-method receiver subclass prefix-subtype check extracted
    // to [`crate::check_type_of_call_class_method_subtype`] (chunk 309).
    if crate::check_type_of_call_class_method_subtype::skip(is_class_method, i, arg_ty, param_ty) {
        return true;
    }
    // V3-18 Nullable<T> match wedge extracted to
    // [`crate::check_type_of_call_nullable_match`]
    // (chunk 308).
    if crate::check_type_of_call_nullable_match::matches(param_ty, arg_ty) {
        return true;
    }
    // S133 callback Function subtype carve-out extracted
    // to [`crate::check_type_of_call_callback_subtype`]
    // (chunk 307).
    // The resolving flavor: this site has the class tables, and a
    // `ClassRef` inside the callback's parameter list is below the
    // reach of the top-level resolve above.
    if crate::check_type_of_call_callback_subtype::matches_resolved(
        param_ty,
        arg_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return true;
    }
    // RFC 20260707 chunk 626 — T-11 container widen at the call
    // boundary: an `Array(Any)` param admits any concrete
    // `Array(T)` arg (mirror of check_assignable's T-11 arm).
    // Every lowering call-arg station pairs this admit with
    // `emit_arr_mark_kind` per the RFC §2 protocol so the
    // callee's kind-aware Arr<Any> readers decode the raw block.
    if matches!(param_ty, Type::Array(el) if matches!(**el, Type::Any))
        && matches!(arg_ty, Type::Array(_))
    {
        return true;
    }
    // RFC 20260802-any-arg-typed-param-mono — the REVERSE direction:
    // an `Array(Any)` arg into a typed `Array(T)` param is TS any-
    // assignability (`var seq = []; seq.push(1); f(seq)` against
    // `f(xs: number[])`). Admission is monomorph-backed: the site is
    // recorded and `check_monomorph_any_widen` retargets it to a
    // callee clone whose param is widened to `any[]`, so a typed
    // reader never sees the NaN-boxed block. Gated to plain-Ident
    // non-generic non-generator user FnDecls without rest params —
    // the only shape the clone+retarget lane handles; everything
    // else stays loud.
    if arr_any_into_typed_arr_param(checker, ast, eid, callee, arg_ty, param_ty, i) {
        return true;
    }
    // Chunk 641 — an empty `[]` literal argument has no element
    // to infer from and types Array(Any); any Array(T) param
    // admits it contextually (`take([])`, `new N([])` — bun
    // accepts, l17b/l17e). The lowering side pairs this with a
    // param-typed empty alloc (`try_lower_empty_array_arg`) so
    // the callee never sees a FLAG_ARR_ANY block behind a typed
    // param slot.
    if matches!(param_ty, Type::Array(_))
        && matches!(
            ast.get_expr(*arg_id),
            crate::ast::Expr::Array(els) if els.is_empty()
        )
    {
        return true;
    }
    // RFC 20260708-spread-call chunk 2a — TS any-assignability
    // at the call boundary: an Any arg into a scalar / String
    // param is admitted, paired with a caller-side coerce at the
    // lowering lanes. Originally gated to plain Ident callees;
    // r359 extends it to `__cm_` class-method idents and
    // Member-callee shapes — both route through
    // `coerce_args_by_param_tys` (the terminal serves `__cm_`
    // direct calls, and the struct-method dispatch shares the
    // same contract since RFC 20260714 刀 1), so the coerce hook
    // the original gate was missing exists now (probe: plain-fn
    // `f(u)` coerced while `new A().m(u)` stayed a loud reject —
    // bun accepts both). Heap-typed params take the monomorph lane
    // below instead — there is still no caller-side Any→heap unbox
    // helper, and widening the CALLEE's slot removes the need for
    // one.
    if matches!(arg_ty, Type::Any)
        && matches!(
            param_ty,
            Type::Number | Type::String | Type::Boolean | Type::BigInt
        )
        && matches!(
            ast.get_expr(*callee),
            crate::ast::Expr::Ident(_) | crate::ast::Expr::Member { .. }
        )
    {
        return true;
    }
    // r380 — the heap-typed half of that same rule, in
    // [`any_into_heap_param`] (it records a widen site, so it wants
    // its own frame).
    if any_into_heap_param(checker, ast, eid, callee, arg_ty, param_ty, i) {
        return true;
    }
    // Chunk 762 — struct-param Nullable-field covariance wedge:
    // `{ inner: { a: 3 } }` / `{ inner: undefined }` into a declared
    // `{ inner?: Inner }` param, riding the same assignability
    // lattice the let-decl lane uses. See
    // [`crate::check_type_of_call_struct_field_covariance`].
    crate::check_type_of_call_struct_field_covariance::matches(checker, param_ty, arg_ty)
}
