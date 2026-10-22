//! The container wedges of the per-argument admit predicate (a Map, an
//! array, a promise of `any` into a typed parameter), split out of
//! `arg_admitted.rs` in the order they ran there.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_type_of_call::any_widen::arr_any_into_typed_arr_param;
use crate::check_type_of_call::any_widen_promise::any_promise_into_promise_param;

/// Does a container argument fit its parameter by one of the wedges
/// below? `param_ty` / `arg_ty` arrive with class names resolved.
#[allow(clippy::too_many_arguments)]
pub(super) fn container_admitted(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    i: usize,
    param_ty: &Type,
    arg_ty: &Type,
    arg_id: &ExprId,
) -> bool {
    // `Map<K, V>` is erased below the checker, so a Map argument fits
    // a Map param by the payload rule assignment uses (`new Map()` —
    // `Map<any, any>` — into `m: Map<string, number>`).
    if matches!((param_ty, arg_ty), (Type::Map(..), Type::Map(..)))
        && crate::check_assignable::is_assignable_to(param_ty, arg_ty)
    {
        return true;
    }
    // An `Array(Any)` param admits any concrete `Array(T)` arg, as
    // assignment does. Every lowering call-arg station pairs this
    // admit with `emit_arr_mark_kind`, so the callee's kind-aware
    // Arr<Any> readers decode the raw block.
    // An optional or `| undefined` parameter of that type widens the
    // same way: its slot is the same array pointer, null when absent.
    let arr_param = match param_ty {
        Type::Nullable(inner) => inner.as_ref(),
        t => t,
    };
    if matches!(arr_param, Type::Array(el) if matches!(**el, Type::Any))
        && matches!(arg_ty, Type::Array(_))
    {
        return true;
    }
    // The reverse direction: an `Array(Any)` arg into a typed
    // `Array(T)` param is TS any-assignability
    // (`var seq = []; seq.push(1); f(seq)` against
    // `f(xs: number[])`). Admission is monomorph-backed: the site is
    // recorded and `check_monomorph_any_widen` retargets it to a
    // callee clone whose param is widened to `any[]`, so a typed
    // reader never sees the NaN-boxed block. Gated to plain-Ident
    // non-generic non-generator user FnDecls without rest params —
    // the only shape the clone+retarget lane handles; everything
    // else stays loud.
    if arr_any_into_typed_arr_param(checker, ast, eid, callee, arg_ty, param_ty, i)
        || any_promise_into_promise_param(checker, ast, eid, callee, arg_ty, param_ty, i)
    {
        return true;
    }
    // An empty `[]` literal argument has no element to infer from
    // and types Array(Any); any Array(T) param admits it
    // contextually (`take([])`, `new N([])`). The lowering side
    // pairs this with a param-typed empty alloc
    // (`try_lower_empty_array_arg`) so
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
    false
}
