//! Cascade segment 1/6 of [`crate::check_type_of_call::check`]'s
//! early-route chain. Wedge order preserved verbatim from the
//! pre-split cascade; segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings — relative order of
//! every arm is unchanged.
//!
//! Covers: cm_demote / in_op / promise_then / global_ctors / number_parse /
//! promise_static / process_on / promise_all / object_static / arr_flat /
//! array_from / date_utc / reduce_1arg

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // Rotation 371 — `__superbuiltin__<m>(this, args…)` is the
    // desugared `super.m()` of a builtin-heritage subclass method
    // (desugar_classes_super): the callee is a runtime re-dispatch,
    // not a program identifier, so admit it here (args still
    // typecheck) before the generic ident route would refuse the
    // unknown name. Answers Any — the builtin surface's verdict is
    // a runtime fact.
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n.starts_with("__superbuiltin__")
    {
        for &a in args.iter() {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    // Name-based class-method rewrite vs builtin-container
    // receiver — decision + alt typecheck live in cm_demote.rs.
    if let Some(demoted) = checker.try_demote_cm_rewrite(ast, eid, args) {
        return Some(demoted);
    }
    // Any-method-call RFC 20260704 — a method call whose receiver
    // types as `any` is legal per TS (`any` absorbs every call)
    // and answers `any`; lowering routes it to the runtime method
    // dispatcher. Must run before every typed dispatch arm below —
    // their name-based matches would otherwise claim the call and
    // typecheck the receiver as a concrete type. (After cm_demote:
    // class instances behind `any` keep their existing rewrite.)
    if let Expr::Member { obj, .. } = ast.get_expr(*callee)
        && matches!(checker.type_of(ast, *obj), Ok(Type::Any))
    {
        for a in args {
            if let Err(e) = checker.type_of(ast, *a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    // RFC 20260713-array-proto-residual blade 2 — the
    // `<any>.toString.call(x)` family: the `(Any, "toString" /
    // "valueOf")` member sugar arms type the read as a concrete
    // Function so its DIRECT call answers String, but the read is
    // still a runtime any cell (a reified builtin / user closure),
    // so the Function.prototype surfaces (.call / .apply / .bind)
    // on it stay any-dispatched. Mirrored by the ssa_lower
    // any-method-call gate.
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && matches!(name.as_str(), "call" | "apply" | "bind")
        && matches!(checker.type_of(ast, *obj), Ok(Type::Function(..)))
        && let Expr::Member { obj: inner, .. } = ast.get_expr(*obj)
        && matches!(checker.type_of(ast, *inner), Ok(Type::Any))
    {
        for a in args {
            if let Err(e) = checker.type_of(ast, *a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    if let Some(r) = try_builtin_mv_fn_surface(checker, ast, callee, args) {
        return Some(r);
    }
    // RFC 20260719-fn-tostring-source B4b/B6a — `f.toString()` on a
    // fn-typed receiver answers String; lowering
    // (`ssa_lower_call_fn_tostring`) folds the type-erased source at
    // compile time for a top-level fn ident, and routes every other
    // Function-typed value (closure binding / fn param / fn-typed
    // field) through the runtime erased-source kernels by SSA repr.
    // The two gates key on the same truths (top-level FnDecl walk /
    // this Function typing recorded in expr_types) so admitted and
    // lowered shapes can't drift. AFTER the any-receiver arm: an
    // any-held fn keeps the runtime dispatch.
    // (`toLocaleString` rides the same arm — §20.2.3.5 makes it
    // toString's answer.)
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && (name == "toString" || name == "toLocaleString")
        && args.is_empty()
        && (matches!(checker.type_of(ast, *obj), Ok(Type::Function(..)))
            || (matches!(ast.get_expr(*obj), Expr::Ident(f) if ast
                .stmts
                .iter()
                .any(|s| matches!(s, crate::ast::Stmt::FnDecl { name: n, .. } if n == f)))))
    {
        return Some(Ok(Type::String));
    }
    if let Some(r) = try_fn_value_call(checker, ast, eid, callee, args) {
        return Some(r);
    }
    // RFC C4+ — a bare call whose callee itself types as `any`
    // (`f(1)` on an any-held closure) is legal per TS and answers
    // `any`; lowering routes it to the runtime closure dispatch.
    // Member callees stay above / with the typed arms (getter-as-
    // callee is a recorded C4+ boundary).
    if !matches!(ast.get_expr(*callee), Expr::Member { .. })
        && matches!(checker.type_of(ast, *callee), Ok(Type::Any))
    {
        for a in args {
            if let Err(e) = checker.type_of(ast, *a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    // T-45 — synthetic call from parser for binary `in`
    // operator: `__torajs_in_op(key, obj)`. Wedge extracted to
    // [`crate::check_type_of_call_in_op`] (chunk 296).
    if let Some(r) = crate::check_type_of_call_in_op::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // §13.10 ergonomic brand check (`#x in o`) — the priv sibling.
    if let Some(r) = crate::check_type_of_call_in_op::try_match_priv(checker, ast, callee, args) {
        return Some(r);
    }
    // Promise<T>.then / .catch early-route arms (T-19.l 2-arg
    // shape, T-19.o heterogeneous T→U + P10.7 Promise<Any>,
    // P10.2-A1.1 Promise<Undefined>, P10.2-A4 Promise<Array<U>>)
    // — see [`crate::check_type_of_call_promise_then`] (chunk
    // 207 — first sub-batch of check_type_of_call.rs per-shape
    // decomposition). All 4 patterns must run BEFORE the
    // regular method-table dispatch because the table's
    // static signature fixes arg count and inner-T constraint.
    // `.finally` is not handled there — its `() => void` shape is
    // the table arm's, and every OTHER return shape is the arm
    // below's (§27.2.5.3 declares `onFinally` as `() => any`).
    if let Some(r) = crate::check_type_of_call_promise_then::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    if let Some(r) =
        crate::check_type_of_call_promise_finally::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // Global bare-Ident ctor / coercion call shapes
    // (`fetch(url)` / `Number|String|Boolean(x)` callable
    // coercion / `BigInt(value)` ctor / `Symbol(desc?)`) —
    // see [`crate::check_type_of_call_global_ctors`] (chunk
    // 208 — second sub-batch of check_type_of_call.rs per-
    // shape decomposition). All 4 are early-route Ident
    // callee shapes that must run BEFORE the regular
    // method-table / general-call dispatch.
    if let Some(r) = crate::check_type_of_call_global_ctors::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // Number.parseInt + Number.parseFloat early-route arms —
    // see [`crate::check_type_of_call_number_parse`] (chunk
    // 209 — third sub-batch). Both need early-route handling
    // because the regular static-method table fixes arity in
    // ways the spec ignores (parseInt 1-arg, parseFloat 0-arg
    // both had to circumvent the unified arity gate).
    if let Some(r) = crate::check_type_of_call_number_parse::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // T-15.g.5 / T-19.b/d/f — `Promise.resolve(v)` /
    // `Promise.reject(v)` with arg-type-driven return
    // inference. Extracted to `check/promise_static.rs`
    // (2026-06-03, P10.5-A2 prereq).
    if let Some(r) = checker.check_promise_resolve_reject_static(ast, *callee, args) {
        return Some(r);
    }
    // P10.5-A4 — `process.on('unhandledRejection', cb)`.
    // Extracted to `check/process_on.rs`.
    if let Some(r) = checker.check_process_on(ast, *callee, args) {
        return Some(r);
    }
    // Promise.all / .race / .any / .allSettled fan-in static
    // methods — see [`crate::check_type_of_call_promise_all`]
    // (chunk 210 — fourth sub-batch). Input is
    // Array<Promise<T>>; result varies per method
    // (.all → Promise<T[]> / .race | .any → Promise<T> /
    // .allSettled → Promise<{status,value}[]>).
    if let Some(r) = crate::check_type_of_call_promise_all::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // Object.assign / Object.values static-method early-route
    // arms — see [`crate::check_type_of_call_object_static`]
    // (chunk 211 — fifth sub-batch). Object.assign requires
    // target+sources identical struct types in this subset;
    // Object.values is polymorphic over Array / String / Any /
    // struct receivers.
    if let Some(r) = crate::check_type_of_call_object_static::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    route_tail(checker, ast, callee, args)
}

/// Tail of the segment, split off when the `.finally` arm pushed
/// `try_route` past the 200-line function limit. The seam is
/// mechanical like the segment boundaries themselves — consecutive
/// arms, relative order unchanged — and every arm here is
/// receiver-name-based, so nothing above it can be claimed by
/// running it second.
fn route_tail(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // `arr.flat(N)` literal-depth early-route arm — see
    // [`crate::check_type_of_call_arr_flat`] (chunk 212 —
    // sixth sub-batch). Peels `Array<>` layers from the
    // receiver's element type when the depth arg is a
    // Number / `Infinity` / `undefined` literal. The 0-arg
    // `xs.flat()` shape uses the regular method-table arm.
    if let Some(r) = crate::check_type_of_call_arr_flat::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // Array.from(iter, mapFn?) polymorphic early-route arm —
    // see [`crate::check_type_of_call_array_from`] (chunk 213
    // — seventh sub-batch). 1-arg receiver-polymorphic over
    // String / Array / Set; 2+ arg `Array.from(iter, mapFn,
    // thisArg?)` result is Array<mapFn ret>.
    if let Some(r) = crate::check_type_of_call_array_from::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S153 — `Date.UTC(...)` 1-6 arg overload early-route arm —
    // see [`crate::check_type_of_call_date_utc`] (chunk 214 —
    // eighth sub-batch). Per ES §21.4.2.21 trailing-defaults
    // overloads; the 7-arg form keeps using the static-sig path
    // unchanged.
    if let Some(r) = crate::check_type_of_call_date_utc::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // `xs.reduce(cb)` / `xs.reduceRight(cb)` 1-arg overload
    // early-route arm — see
    // [`crate::check_type_of_call_reduce_1arg`] (chunk 215 —
    // ninth sub-batch). ES §23.1.3.24 / §23.1.3.25 init-value-
    // defaulting form; the 2-arg form is covered by the
    // static-sig arm.
    if let Some(r) = crate::check_type_of_call_reduce_1arg::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    None
}

/// L3b ⑥ — `Function.prototype.call` / `.apply` on a statically
/// fn-typed VALUE (`const f = add; f.call(u, 2, 3)` /
/// `f.apply(u, [2, 3])`): the named-fn form never reaches here (the
/// chunk-138 AST desugar rewrote it), and an any-held fn keeps the
/// runtime dispatch (the any-receiver arm runs first). The thisArg
/// types for effect then drops (the desugar's no-this subset rule);
/// the remaining args forward to the general fn-call admit AGAINST
/// THE ORIGINAL eid, so its arity gate / per-arg subtype loop /
/// arity-pad recording all key exactly like the lowering wedge's
/// replayed value-callee call (`ssa_lower_call_fn_call_value`, same
/// eid + rest args). `apply` admits the LITERAL argsArray form only
/// — the chunk-138 desugar's own bound; a runtime array needs a
/// variadic spread substrate, so that shape keeps its loud reject.
/// RFC 20260725-str-method-value-reify — `.call` / `.apply` /
/// `.bind` on a reified String-receiver method value: a registered
/// binding (`const m = s.slice; m.call(s, 1)`) or the inline Member
/// form (`s.slice.call(x, 1)`). Any-dispatched so the member
/// table's fixed-arity sig never rejects the optional-argument
/// forms — the runtime re-dispatch is spec-exact.
fn try_builtin_mv_fn_surface(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if !matches!(name.as_str(), "call" | "apply" | "bind")
        || !matches!(checker.type_of(ast, *obj), Ok(Type::Function(..)))
        || !(is_builtin_mv_read(checker, ast, *obj) || is_ns_static_read(ast, *obj))
    {
        return None;
    }
    for a in args {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}

/// A reified builtin-method value (RFC 20260725-str-method-value-
/// reify): a binding the let-decl marked `builtin_mv`, or the
/// inline `s.slice` Member form (receiver types to a builtin
/// prototype family, member types Function, name interns to a
/// builtin mid with a meta row).
fn is_builtin_mv_read(checker: &mut Checker, ast: &Ast, obj: ExprId) -> bool {
    match ast.get_expr(obj) {
        Expr::Ident(n) => checker.lookup(n).is_some_and(|info| info.builtin_mv),
        Expr::Member { obj: inner, name } => {
            let recv_ok = checker
                .type_of(ast, *inner)
                .is_ok_and(|t| crate::ssa_lower_member::mv_family_of_checker_ty(&t).is_some());
            if !recv_ok {
                return false;
            }
            let mid = torajs_rc::any_method_id(name);
            mid != torajs_rc::ANY_METHOD_UNKNOWN && torajs_rc::any_method_meta(mid).is_some()
        }
        _ => false,
    }
}

/// A reified namespace-static read (`Array.from` / `Math.max` — the
/// intern-table truth the lowering bakes a cell for). Its
/// `.call/.apply/.bind` surface is any-dispatched (RFC
/// 20260808-construct-channel B6 刀 2): the cell's runtime dispatch
/// is spec-exact — recv-first ids read the thisArg, receiver-less
/// ids ignore it per their spec — while the legacy static sig this
/// arm preempts would reject the polymorphic forms.
fn is_ns_static_read(ast: &Ast, obj: ExprId) -> bool {
    matches!(ast.get_expr(obj), Expr::Member { obj: ns, name: m }
        if matches!(ast.get_expr(*ns), Expr::Ident(n)
            if torajs_rc::ns_static::ns_static_id(n, m) >= 0))
}

fn try_fn_value_call(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return None;
    };
    if (name != "call" && name != "apply") || args.is_empty() {
        return None;
    }
    if !matches!(checker.type_of(ast, *obj), Ok(Type::Function(..))) {
        return None;
    }
    let rest: Vec<ExprId> = if name == "call" {
        args[1..].to_vec()
    } else {
        match args.len() {
            // §20.2.3.1 step 2 — an absent argArray means no args
            // (rotation 261; the fn-value lowering's bound matches).
            1 => Vec::new(),
            2 => {
                let Expr::Array(els) = ast.get_expr(args[1]) else {
                    return None;
                };
                els.clone()
            }
            _ => return None,
        }
    };
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    Some(super::general::general_call(checker, ast, eid, obj, &rest))
}
