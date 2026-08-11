//! `Array<T>.{map,filter,forEach,reduce,reduceRight}(cb)` and (rotation
//! 364) the predicate family × argv-face callback
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
    if !matches!(
        m_name.as_str(),
        "map"
            | "filter"
            | "forEach"
            | "reduce"
            | "reduceRight"
            | "find"
            | "findLast"
            | "findIndex"
            | "findLastIndex"
            | "some"
            | "every"
    ) || args.is_empty()
    {
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
    // Reduce needs the seed's type BEFORE the trailing loop below
    // types it (same expr, memoized — no double side effects).
    let seed_ty = if matches!(m_name.as_str(), "reduce" | "reduceRight") {
        match args.get(1) {
            Some(&s) => match checker.type_of(ast, s) {
                Ok(t) => Some(t),
                Err(e) => return Some(Err(e)),
            },
            None => None,
        }
    } else {
        None
    };
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
        // Rotation 364 — the predicate family (short-circuit iter
        // lane): the callback's return only feeds ToBoolean, so the
        // result types mirror the Void-cb wedge's — some/every →
        // Boolean, findIndex/findLastIndex → Number, find/findLast →
        // T with the documented sentinel not-found subset semantics.
        "some" | "every" => Some(Ok(Type::Boolean)),
        "findIndex" | "findLastIndex" => Some(Ok(Type::Number)),
        "find" | "findLast" => Some(Ok((**elem).clone())),
        // Mirror of the lane's `resolve_acc_ty`: the accumulator
        // slot is the callback's return type, widened to Any when
        // the seed (2-arg) or the element (1-arg, §23.1.3.24 step
        // 8's initial accumulator) doesn't share it — the checker's
        // answer and the slot's SSA type must agree or a typed
        // reader sees the other union member's raw bits.
        "reduce" | "reduceRight" => {
            if !matches!(
                **aret,
                Type::Number | Type::String | Type::Boolean | Type::Any | Type::Void
            ) {
                return None;
            }
            let acc = if matches!(**aret, Type::Void | Type::Any) {
                Type::Any
            } else {
                let first = seed_ty.unwrap_or_else(|| (**elem).clone());
                if first == **aret {
                    (**aret).clone()
                } else {
                    Type::Any
                }
            };
            Some(Ok(acc))
        }
        _ => unreachable!(),
    }
}
