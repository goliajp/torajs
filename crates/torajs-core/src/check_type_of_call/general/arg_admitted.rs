//! The per-argument admit predicate of the general call checker, split
//! out of `general.rs`.

use super::container_admit::container_admitted;
use super::defaulted_param::nullable_into_defaulted_param;
use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_type_of_call::any_widen::{
    any_into_heap_param, arr_any_into_typed_arr_param, as_cast_inner_ty,
    hidden_any_into_scalar_param,
};
use crate::check_type_of_call::fn_face::{box_census_arg, fn_face_into_fn_param, held_face};

/// Per-arg admit predicate — strict equality plus the accumulated
/// carve-out wedges (chunk 762 extraction; the wedge stack had
/// grown the loop body past the 200-line fn limit).
#[allow(clippy::too_many_arguments)]
pub(super) fn arg_admitted(
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
    // only an argument the checker already types as the parameter (or as
    // any) rides the as-passed lane; `id(d[1] + "x")` into a number slot
    // stays the type error it is
    if (arg_ty == param_ty || arg_ty == &Type::Any)
        && hidden_any_into_scalar_param(checker, ast, eid, callee, arg_id, param_ty, i)
    {
        return true;
    }
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
    // a binding typed by its annotation that holds a value of another
    // face hands the callee that face (`fn_face::held_face`)
    if let Some(face) = held_face(checker, ast, *arg_id)
        && fn_face_into_fn_param(checker, ast, eid, callee, &face, param_ty, i)
    {
        return true;
    }
    // a census slot handed on: the parameter holds what it holds
    box_census_arg(checker, ast, (*callee, i), (param_ty, *arg_id));
    if param_ty == &Type::Any || arg_ty == param_ty {
        return true;
    }
    if let Some(fits) = crate::iface_witness::class_admit(param_ty, arg_ty) {
        return fits;
    }
    if nullable_into_defaulted_param(ast, callee, i, param_ty, arg_ty) {
        return true;
    }
    if is_class_method && i == 0 && crate::check::nullable_receiver_admits(param_ty, arg_ty) {
        return true;
    }
    // a function value of another face: the clone lane re-spells the
    // callee's param in the value's face (`check_type_of_call::fn_face`)
    if crate::check_type_of_call::fn_face::fn_face_into_fn_param(
        checker, ast, eid, callee, arg_ty, param_ty, i,
    ) {
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
    if rest_value_into_boxed_param(ast, callee, i, param_ty, arg_ty) {
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
    if container_admitted(checker, ast, eid, callee, i, param_ty, arg_ty, arg_id) {
        return true;
    }
    if any_into_heap_param(checker, ast, eid, callee, arg_ty, param_ty, i) {
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
    // above instead — there is still no caller-side Any→heap unbox
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
    // Chunk 762 — struct-param Nullable-field covariance wedge:
    // `{ inner: { a: 3 } }` / `{ inner: undefined }` into a declared
    // `{ inner?: Inner }` param, riding the same assignability
    // lattice the let-decl lane uses. See
    // [`crate::check_type_of_call_struct_field_covariance`].
    crate::check_type_of_call_struct_field_covariance::matches(checker, param_ty, arg_ty)
}

/// Rotation 365 fn-arg track — a rest-tail closure value (an
/// argv-face fn-expr's public type) admits into a declared
/// closure-typed param ONLY where the argv tier marked that exact
/// (fn, param) boxed (`ast.argv_boxed_params`): the SSA variadic
/// registration then routes the param's direct calls through the
/// boxed dual entry, which serves the reshaped signature the
/// rest-tail type advertises. The return types still pair (a Void
/// param slot discards any return).
fn rest_value_into_boxed_param(
    ast: &Ast,
    callee: &ExprId,
    i: usize,
    param_ty: &Type,
    arg_ty: &Type,
) -> bool {
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
    false
}
