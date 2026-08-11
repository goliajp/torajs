//! `Array<T>.{map,filter,forEach}(cb)` × argv-face inline callback
//! wedge (rotation 363) — the callback is an anonymous fn-expr whose
//! body reads `arguments` values, so the argv-face collector
//! reshaped it and the checker's public type for it is the variadic
//! `(...args: any[]) => R` (see `check_type_of_fn`). The
//! method-table arm spells the callback slot `(T, number, any[]) =>
//! R` and rejects that shape outright; this wedge admits it for
//! exactly the channels whose inline loop routes argv-face callees
//! through the boxed variadic call (`emit_variadic_boxed_call` —
//! the adapter feeds real argc + argv into the synthetic params).
//!
//! Result types keep the lanes honest:
//! - `map` answers `Array<R>` for primitive `R` (Number / String /
//!   Boolean / Any) — the hetero-map posture: an `arguments[0]`
//!   body returns `Any`, and folding it into the receiver's `T[]`
//!   would hand typed readers NaN-boxed bits. Non-primitive `R`
//!   falls through to the method-table arm's loud reject.
//! - `filter` answers `Array<T>` — kept elements come from the
//!   source; the callback's return only feeds ToBoolean.
//! - `forEach` answers `Void` — the return is discarded.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "map" | "filter" | "forEach") || args.is_empty() {
        return None;
    }
    // The two shapes the SSA lanes downgrade to the boxed variadic
    // call: the inline closure the collector's HOF-anon arm admitted,
    // and (rotation 363 knife 3) a binding the safe-chain walk let
    // through to the callback slot.
    let is_argv_cb = match ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ast.closure_argv_fns.contains(fn_name),
        Expr::Ident(n) => ast.closure_argv_locals.contains(n),
        _ => false,
    };
    if !is_argv_cb {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Function(ps, aret) = &cb_ty else {
        return None;
    };
    if !matches!(ps.as_slice(), [Type::Rest(_)]) {
        return None;
    }
    // S270 — type_of + drop trailing args (thisArg + extras) so
    // side-effect expressions surface their own errors.
    for &t in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, t) {
            return Some(Err(e));
        }
    }
    match m_name.as_str() {
        "map" => {
            if !matches!(
                **aret,
                Type::Number | Type::String | Type::Boolean | Type::Any
            ) {
                return None;
            }
            Some(Ok(Type::Array(aret.clone())))
        }
        "filter" => Some(Ok(Type::Array(elem.clone()))),
        "forEach" => Some(Ok(Type::Void)),
        _ => unreachable!(),
    }
}
