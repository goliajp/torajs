//! The source-array parameter of a builtin array callback.
//!
//! The builtin callback faces spell the third argument (§23.1.3,
//! «kValue, k, O») as `any[]`, the kind-aware view every receiver can
//! hand. A callback may declare it with an element type instead —
//! `ps.forEach((p: P, i: number, arr: P[]) => …)` — and what decides
//! whether that fits is the receiver it will be handed, not the face:
//! `P[]` fits a `P[]` receiver (the same block arrives) and an `any[]`
//! one (TS admits `any[]` there; the call takes the boxed entry, which
//! checks each element), while `string[]` on a `P[]` receiver does not.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type, resolve_class_ref};

/// `arg_ty` with every source-array parameter the receiver can hand it
/// respelled as the face's own `any[]`, so the ordinary callback match
/// can admit it; `None` when there is nothing to respell (a parameter
/// whose element the receiver cannot hand is left as written and the
/// match refuses it).
pub(super) fn face_view(
    checker: &Checker,
    ast: &Ast,
    callee: ExprId,
    param_ty: &Type,
    arg_ty: &Type,
) -> Option<Type> {
    let Expr::Member { obj, .. } = ast.get_expr(callee) else {
        return None;
    };
    let Some(Type::Array(recv_elem)) = checker.expr_types.get(obj) else {
        return None;
    };
    let (Type::Function(face_ps, _), Type::Function(arg_ps, arg_ret)) = (param_ty, arg_ty) else {
        return None;
    };
    let any_arr = Type::Array(Box::new(Type::Any));
    let resolve = |t: &Type| {
        resolve_class_ref(
            t,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
    };
    let mut changed = false;
    let view = arg_ps
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let (inner, optional) = match a {
                Type::Nullable(x) => (&**x, true),
                x => (x, false),
            };
            let fits = face_ps.get(i) == Some(&any_arr)
                && matches!(inner, Type::Array(el)
                    if **el != Type::Any
                        && (**recv_elem == Type::Any || resolve(el) == resolve(recv_elem)));
            if !fits {
                return a.clone();
            }
            changed = true;
            if optional {
                Type::Nullable(Box::new(any_arr.clone()))
            } else {
                any_arr.clone()
            }
        })
        .collect();
    changed.then(|| Type::Function(view, arg_ret.clone()))
}
