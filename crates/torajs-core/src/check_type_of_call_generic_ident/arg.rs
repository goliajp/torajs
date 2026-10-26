//! The type a generic call's argument unifies its parameter against.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, resolve_class_ref, substitute_typevars};

/// `arg_ty` as the unifier sees it against `param_ty`. Under `subst`
/// (a type argument written out, or one an earlier argument bound),
/// a literal is first put in the declared order it is stored in.
pub(super) fn unify_view(
    checker: &mut Checker,
    ast: &Ast,
    param_ty: &Type,
    subst: Option<&HashMap<String, Type>>,
    arg_id: ExprId,
    arg_ty: Type,
) -> Type {
    let arg_ty = match subst {
        Some(subst) => {
            let declared = substitute_typevars(param_ty, subst);
            crate::check_assignable::in_declared_order(checker, ast, &declared, arg_id, arg_ty)
        }
        None => arg_ty,
    };
    let arg_ty = decay_nullable_arr(param_ty, arg_ty);
    let arg_ty = decay_classref_struct(checker, param_ty, arg_ty);
    unification_ty(ast, arg_id, arg_ty)
}

/// Whether `arg_ty` is a class instance that fills the rows of the
/// interface `param_ty`: the argument is the interface's value as it
/// is, dispatched through the class's witness rows, and binds no type
/// variable (a declared interface has none). The unifier would compare
/// the class's own layout with the interface's member by member and
/// refuse a class with a field of its own.
pub(super) fn class_into_iface(param_ty: &Type, arg_ty: &Type) -> bool {
    crate::iface_witness::class_admit(param_ty, arg_ty) == Some(true)
}

/// The member stores of the object literal arguments into the fields
/// of their parameters under `subst`, recorded as the general call path
/// records them (`crate::check::face_slots::record_objlit_stores`).
pub(super) fn record_objlit_args(
    checker: &mut Checker,
    ast: &Ast,
    params: &[Type],
    args: &[ExprId],
    subst: &HashMap<String, Type>,
) {
    for (param_ty, arg_id) in params.iter().zip(args) {
        if matches!(ast.get_expr(*arg_id), crate::ast::Expr::ObjectLit { .. }) {
            let declared = substitute_typevars(param_ty, subst);
            crate::check::face_slots::record_objlit_stores(checker, ast, &declared, *arg_id);
        }
    }
}

/// RC-4 — a `Nullable<Array<..>>` arg (exec/match result, chunk 598
/// retype) decays to its inner Array against an Array-shaped generic
/// param, mirroring the member/index decay: unification never binds
/// a TypeVar to a Nullable wrapper. The runtime null stays guarded —
/// ssa_lower_call_terminal emits `__torajs_arr_null_check` on
/// nullable-arr args at retargeted call sites.
fn decay_nullable_arr(param_ty: &Type, arg_ty: Type) -> Type {
    if matches!(param_ty, Type::Array(_))
        && let Type::Nullable(inner) = &arg_ty
        && matches!(**inner, Type::Array(_))
    {
        return (**inner).clone();
    }
    arg_ty
}

/// A class instance against a struct-shaped generic param — the
/// `ClassRef` doc's "dereference at consumer sites" applied to
/// unification, plus width subtyping over the flattened layout.
/// `class NumBox extends Box<number>` inherits `get()` as the
/// generic fn `__cm_Box__get(__this: Box<T>)`: the param resolved
/// to the parent's Struct shape, and the receiver arrives either as
/// `ClassRef("NumBox")` (a top-level call) or as an
/// already-structural `Wide<string>` instantiation (inside a
/// specialization body, where the receiver param's own substituted
/// ann resolved). Either way, when the actual is WIDER than the
/// pattern (a subclass appended its own fields after the flattened
/// parent prefix), unify only the pattern-width prefix — sound
/// because a parent method reads only parent slots, and the unify's
/// own field-NAME check still rejects unrelated shapes.
fn decay_classref_struct(checker: &Checker, param_ty: &Type, arg_ty: Type) -> Type {
    let Type::Struct(p_fields) = param_ty else {
        return arg_ty;
    };
    let resolved = match &arg_ty {
        Type::ClassRef(_) => resolve_class_ref(
            &arg_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        ),
        Type::Struct(_) => arg_ty,
        _ => return arg_ty,
    };
    match resolved {
        Type::Struct(mut a_fields) if a_fields.len() > p_fields.len() => {
            a_fields.truncate(p_fields.len());
            Type::Struct(a_fields)
        }
        other => other,
    }
}

/// The type an argument pins a type PARAMETER to.
///
/// An arity hole the default pad wrote (`Ast::arity_hole_args`) pins
/// `Any`, because the program passed no argument there and the T-28
/// branch in [`try_match`] already answers that question that way for
/// the calls it keeps — an absent trailing argument binds `Any` and
/// pads with ANY_UNDEF. Once the pad started materializing the hole
/// those two lanes disagreed: `function f(x, _ = 0)` called `f()`
/// bound `x`'s implicit typevar to `Any` before the pad could reach
/// the default behind the hole and to `Undefined` after, and an
/// `Undefined`-typed `x` then failed to unify at any generic call
/// taking it beside another type (`sameValue(x, 2)`) — reported by
/// the lowerer as "unknown function", since a call the checker could
/// not infer gets no monomorph retarget.
///
/// Only the UNIFICATION answer moves. The hole's expression type
/// stays `Undefined`, which is what the call-arg conversion reads to
/// box ANY_UNDEF into the slot; typing the expression itself `Any`
/// handed the callee a raw null instead (`m()` on `m(x, y = 5)`
/// printed `null`).
fn unification_ty(ast: &Ast, arg_id: ExprId, arg_ty: Type) -> Type {
    if ast.arity_hole_args.contains(&arg_id) {
        Type::Any
    } else {
        arg_ty
    }
}
