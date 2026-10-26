//! An `any` argument for a fn-typed parameter of a generic function
//! (`const v = objs.f; first(v, 10)` against `first<T>(cb: (t: T) =>
//! T, x: T)`), the generic twin of the any-widen lane's plain-callee
//! case (`check_type_of_call::any_widen::any_into_heap_param`).
//!
//! So is an `any` argument for an object or array parameter
//! (`call<T>(o: J, t: T)` handed `o` of `any`, `J` an interface or an
//! object type): the box may hold a class instance, an object of
//! another layout, an array of another element kind or no object at
//! all, and a slot laid out as the parameter's type reads its fields at
//! that type's offsets. The instantiation takes the parameter as `any`
//! and reads the value as it is (§13.3.2.1).
//!
//! An `any` box carries a callable as its cell, whose typed entry was
//! compiled for the signature the function was written with, not the
//! one the parameter declares; only the cell's boxed entry takes any
//! signature. So neither parameter lane can take it: a `__fn(` slot
//! jumps into the cell header, and a `__cls(` slot calls the typed
//! entry with the declared argument widths. The instantiation such a
//! call names takes that parameter as `any` instead, and its body
//! calls through the boxed entry. Typed callers keep their own
//! instantiation.
//!
//! The marks ride the specialization key past the type arguments
//! (`__anyarg<i>`, and `__anyret` when the call types `any`, see
//! `check_type_of_call_generic_ident::any_fn_ret`), so they name their
//! own instance and never enter the type-variable substitution.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, Type};
use crate::ssa_lower_generics_monomorph::Generics;

const MARK: &str = "__anyarg";
const RET_MARK: &str = "__anyret";

/// Append to the specialization key `key` of the generic call `eid` to
/// `name` its marks: `__anyarg<i>` per fn-typed or object parameter
/// whose argument the checker typed `any`, then `__anyret` when the call
/// itself types `any`. A class factory is left out, since its chain
/// methods specialize under the same key.
pub(super) fn mark(
    key: &mut Vec<String>,
    ast: &Ast,
    eid: ExprId,
    name: &str,
    generics: &Generics,
    checker: &Checker,
) {
    let Expr::Call { callee, args } = ast.get_expr(eid) else {
        return;
    };
    let (Expr::Ident(n), Some(Type::Function(ptys, _))) =
        (ast.get_expr(*callee), checker.globals.get(name))
    else {
        return;
    };
    if n != name || name.starts_with("__new_") || !generics.contains_key(name) {
        return;
    }
    let len = key.len();
    key.extend(
        ptys.iter()
            .zip(args)
            .enumerate()
            .filter(|(_, (p, a))| checker.expr_types.get(a).is_some_and(|a| takes_any(p, a)))
            .map(|(i, _)| format!("{MARK}{i}")),
    );
    if key.len() > len && checker.widen_ret_any.contains(&eid) {
        key.push(RET_MARK.to_string());
    }
}

/// Whether a parameter typed `p` takes an argument typed `a` as `any`:
/// an `any` value for a function slot, an object one or an array one,
/// and an array of `any` values for an array slot of a declared element
/// type (a type variable binds `any` instead). A class slot is none: an
/// `any` value written into it is checked and copied into an instance
/// of the class, which this lane does not do.
pub(crate) fn takes_any(p: &Type, a: &Type) -> bool {
    match (p, a) {
        (Type::Function(..) | Type::Struct(_) | Type::Array(_), Type::Any) => true,
        (Type::Array(pe), Type::Array(ae)) => {
            **ae == Type::Any && !matches!(**pe, Type::Any | Type::TypeVar(_))
        }
        _ => false,
    }
}

/// Widen the parameters (and the return) the marks in `key` name to
/// `any`, in the specialization's annotations and in its registered
/// signature. A type argument's annotation never takes a mark's
/// spelling, so the whole key is scanned.
pub(super) fn widen(c: &mut Checker, spec: &mut Stmt, key: &[String]) {
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        ..
    } = spec
    else {
        return;
    };
    let mut sig = c.globals.get_mut(name.as_str());
    if key.iter().any(|m| m == RET_MARK) {
        *return_type = Some("any".to_string());
        if let Some(Type::Function(_, rty)) = sig.as_deref_mut() {
            **rty = Type::Any;
        }
    }
    for i in key
        .iter()
        .filter_map(|m| m.strip_prefix(MARK)?.parse::<usize>().ok())
    {
        if let Some(p) = params.get_mut(i) {
            p.type_ann = Some("any".to_string());
        }
        if let Some(Type::Function(ptys, _)) = sig.as_deref_mut()
            && let Some(t) = ptys.get_mut(i)
        {
            *t = Type::Any;
        }
    }
}
