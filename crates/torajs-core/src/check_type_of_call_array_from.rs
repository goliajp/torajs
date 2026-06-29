//! `Array.from(iter, mapFn?)` polymorphic early-route arm
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 213 — seventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! `Array.from` is polymorphic over receiver and arity in
//! ways the static-method table can't express:
//!
//! - S132 — `Array.from(arrLike)` 1-arg: receiver-polymorphic
//!   over String / typed Array / Set. The static fn-sig at
//!   ~2993 in check.rs is fixed to `(String) → Array<String>`
//!   for compile-time arrayLike-from-string lowering; for a
//!   typed Array<T> input, return Array<T> directly so a
//!   shallow-copy emit can pick it up.
//! - S141 — `Array.from(set)` per ES §23.1.2.1 + §24.2.3.13
//!   (Set iterator protocol yields values). Set storage tags
//!   each entry as Any (untyped), so result is `Array<Any>`.
//! - S151 — `Array.from(iter, mapFn)` 2-arg: mapFn shape
//!   `(elem) → R` or `(elem, idx) → R`; result is `Array<R>`
//!   where R = mapFn's ret. iter accepts the same 3
//!   substrates as 1-arg.
//! - S275 — widen `args.len() == 2` to `>= 2` per ES
//!   §23.1.2.1: spec accepts `Array.from(iter, mapFn,
//!   thisArg)`; trailing args past thisArg silent-drop. tora's
//!   closures don't bind `this`, so thisArg + further
//!   trailing are typecheck-and-dropped.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch within the matched 2+ arg path, `None`
//! when callee isn't `Array.from` OR when the 1-arg path
//! falls through to the static-sig path (String overload).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "from"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Array"
    {
        if args.len() == 1 {
            let arg_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Type::Array(elem) = &arg_ty {
                return Some(Ok(Type::Array(elem.clone())));
            }
            // S141 — `Array.from(set)` per ES §23.1.2.1 + §24.2.3.13
            // (Set iterator protocol yields values). Set storage
            // tags each entry as Any (untyped), so the result is
            // `Array<Any>` — print/spread/eq paths already handle
            // Any-tagged elements uniformly.
            if matches!(arg_ty, Type::Set) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // Fall through to the static-sig path; the String
            // overload already covered there throws a sensible
            // arity / type mismatch otherwise.
        } else if args.len() >= 2 {
            // S151 — `Array.from(iter, mapFn)` per ES §23.1.2.1.
            // mapFn shape `(elem) → R` or `(elem, idx) → R`;
            // result type is `Array<R>` where R = mapFn's ret.
            // iter accepts the same three substrates as 1-arg
            // (String / typed Array / Set); the actual ssa-lower
            // path picks 1-arg substrate then runs a map loop.
            //
            // S275 — widen `args.len() == 2` to `>= 2` per ES
            // §23.1.2.1: spec accepts `Array.from(iter, mapFn,
            // thisArg)`; trailing args past thisArg silent-
            // drop. tora's closures don't bind `this`, so
            // thisArg + further trailing are typecheck-and-
            // dropped; ssa_lower mirror evals-and-drops.
            let arg_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            match &arg_ty {
                Type::String | Type::Array(_) | Type::Set => {}
                _ => {
                    return Some(Err(format!(
                        "Array.from(iter, mapFn): iter must be string, Array, or Set; got {arg_ty:?}"
                    )));
                }
            }
            let fn_ty = match checker.type_of(ast, args[1]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            let ret_ty = match &fn_ty {
                Type::Function(_, ret) => (**ret).clone(),
                _ => {
                    return Some(Err(format!(
                        "Array.from: 2nd arg must be a function, got {fn_ty:?}"
                    )));
                }
            };
            for &a in &args[2..] {
                if let Err(e) = checker.type_of(ast, a) {
                    return Some(Err(e));
                }
            }
            return Some(Ok(Type::Array(Box::new(ret_ty))));
        }
    }
    None
}
