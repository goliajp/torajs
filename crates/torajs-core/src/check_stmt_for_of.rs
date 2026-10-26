//! `Stmt::ForOf { var_name, var_type_ann, src_ident, i_ident,
//! elem_expr, body }` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::ForOf` arm as
//! chunk-106 of the check_stmt decomp. 9th check_stmt sibling.
//!
//! P5.3 generic for-of. The parser hoists src to a fresh Ident and
//! pre-builds `elem_expr = src[i]`. Typing the var_name binding
//! goes through `Expr::Index` lowering on elem_expr — which already
//! infers the right element type per source shape (`Array<T>` value
//! = T, `String[i]` = String, dynobj-backed `Any[i]` = Any). We
//! also declare `i_ident` as a Number local so the synthetic
//! counter typechecks.
//!
//! - **P5.3 Phase B exception** — when src has `Type::Struct` (a
//!   class instance), the protocol path in ssa_lower bypasses
//!   elem_expr entirely; typing `src[i]` here would error ("can't
//!   index into Struct"). Probe src's type first; if it's a
//!   class-shape Struct, walk the iterator protocol at the type
//!   level (`[Symbol.iterator]()` → iter class → `next()` →
//!   IteratorResult's `value` field) — the same chain ssa_lower
//!   resolves at the SSA layer, so the binding's checked type now
//!   matches its lowered one. Falls back to Any when any hop is
//!   unresolvable (the pre-existing "defer to ssa_lower" posture).
//! - **P6.4c** — same skip for `Type::Map` / `Type::Set` /
//!   `Type::MapIter` / `Type::ArrIter` (handled by
//!   `lower_for_of_map_like`). For Type::Map specifically the
//!   yielded value is `[k, v]` `Array<Any>` (enables destructuring
//!   `for (let [k, v] of m)`); for Set / MapIter / ArrIter the
//!   yield is type-erased Any.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, DiagPush, FnSlot, LocalInfo, Type, resolve_type_ann};

/// The declared class whose instance type is `ty` — the checker-side
/// twin of ssa_lower's alias scan.
///
/// RFC 20260715-nominal-class-identity — a class instance NAMES its
/// class. This used to fall back to matching a bare `Struct`
/// structurally against every registered class, which is exactly how a
/// same-shaped object literal reached a class's members.
fn class_name_of(ast: &Ast, ty: &Type) -> Option<String> {
    match ty {
        Type::ClassRef(n) if ast.class_parents.contains_key(n) => Some(n.clone()),
        _ => None,
    }
}

/// Return type of a declared fn, keeping a `ClassRef` result nominal —
/// the iterator hops below look the next class up BY NAME.
fn fn_ret_ty(checker: &Checker, fn_name: &str) -> Option<Type> {
    let Some(Type::Function(_, ret)) = checker.globals.get(fn_name) else {
        return None;
    };
    Some((**ret).clone())
}

/// Element type yielded by a class-instance for-of source: walk the
/// iterator protocol at the type level, exactly as ssa_lower walks it
/// at the SSA layer (`ssa_lower_for_of_iter_protocol::resolve_iter_plan`).
/// `None` when any hop is missing — the caller then keeps the opaque
/// Any binding rather than raising an error, since ssa_lower owns the
/// real diagnostic for a malformed iterable.
fn class_iter_elem_ty(checker: &Checker, ast: &Ast, src: &Type) -> Option<Type> {
    let src_class = class_name_of(ast, src)?;
    let iter_ty = fn_ret_ty(
        checker,
        &format!("__cm_{src_class}____sym_Symbol_iterator__"),
    )?;
    let iter_class = class_name_of(ast, &iter_ty)?;
    // The step result (`{ value, done }`) is a structural shape, so it
    // is unwrapped — only the class hops above stay nominal.
    let step_ty = crate::check::resolve_class_ref(
        &fn_ret_ty(checker, &format!("__cm_{iter_class}__next"))?,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    let Type::Struct(fields) = step_ty else {
        return None;
    };
    fields
        .iter()
        .find(|(n, _)| n == "value")
        .map(|(_, t)| t.clone())
}

// mirrors the fields of the desugared for-of statement
#[allow(clippy::too_many_arguments)]
pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    var_name: &str,
    var_type_ann: &Option<String>,
    i_ident: &str,
    elem_expr: ExprId,
    body: &Stmt,
    is_await: bool,
) {
    checker.enter_loop(ast, &[elem_expr], body);
    checker.scopes.push(HashMap::new());
    let _ = checker.declare(i_ident.to_string(), LocalInfo::plain(Type::Number));
    let src_kind = if let Expr::Index { obj, .. } = ast.get_expr(elem_expr) {
        checker.type_of(ast, *obj).ok()
    } else {
        None
    };
    let src_is_iter_subset = matches!(
        src_kind,
        Some(Type::Struct(_))
            | Some(Type::ClassRef(_))
            | Some(Type::Map(..))
            | Some(Type::Set)
            | Some(Type::MapIter(..))
            | Some(Type::ArrIter)
    );
    let elem_ty = if src_is_iter_subset {
        match &src_kind {
            Some(Type::Map(..)) => Type::Array(Box::new(Type::Any)),
            Some(Type::MapIter(t)) => (**t).clone(),
            Some(src_struct @ (Type::Struct(_) | Type::ClassRef(_))) => {
                class_iter_elem_ty(checker, ast, src_struct).unwrap_or(Type::Any)
            }
            _ => Type::Any,
        }
    } else {
        match checker.type_of(ast, elem_expr) {
            Ok(t) => t,
            Err(e) => {
                checker.errors.push_err(e);
                Type::Any
            }
        }
    };
    // Hole Z — `for await` awaits each element (§14.7.5.10): a
    // Promise(T) element binds as T; every non-thenable element
    // awaits to itself, so Struct / Object / primitive elements bind
    // unchanged. The elem_expr stays typed Promise(T) in expr_types —
    // ssa_lower's array lane reads that verdict to route the load
    // through promise_get_value.
    let elem_ty = match elem_ty {
        Type::Promise(inner) if is_await => *inner,
        t => t,
    };
    let var_ty = if let Some(ann) = var_type_ann {
        resolve_type_ann(ann, &checker.aliases).unwrap_or(elem_ty)
    } else {
        elem_ty
    };
    // §14.7.5.7 — a `let` head's per-iteration binding is mutable; a
    // `const` head's writes were already rewritten into throws by the
    // parser (`throw_on_const_loop_var_writes`), so nothing here has
    // to refuse them. Ownership of a written variable is the
    // lowering's business (`ssa_lower_for_of_written`).
    // the head of a walk over an array of the element census holds
    // what the elements hold: its calls ride the boxed entry
    let face_elem = matches!(&src_kind, Some(t @ Type::Array(_))
        if crate::check::face_slots::elem_read(checker, t).is_some());
    if face_elem {
        crate::check::face_slots::record_let_store(checker, elem_expr);
    }
    let _ = checker.declare(
        var_name.to_string(),
        LocalInfo {
            fn_slot: face_elem.then_some(FnSlot::Let(elem_expr)),
            decl: Some(FnSlot::Let(elem_expr)),
            ..LocalInfo::plain(var_ty)
        },
    );
    checker.check_stmt(ast, body);
    checker.scopes.pop();
}
