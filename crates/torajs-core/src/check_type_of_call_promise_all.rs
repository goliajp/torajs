//! `Promise.all` / `.race` / `.any` / `.allSettled` static-
//! method early-route arms extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 210 — fourth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Covers the 4 Promise namespace static methods that fan in
//! over an Array<Promise<T>>:
//! - T-17.a — `Promise.all(promises)` → Promise<Array<T>>.
//!   Sync fast-path MVP; input must be all-fulfilled at call
//!   time (pending elements yield a rejected outer Promise).
//! - `Promise.race(promises)` / `.any(promises)` →
//!   Promise<T> (single winner / first-fulfilled).
//! - T-17.c-A3 — `Promise.allSettled(promises)` →
//!   `Promise<{status: string, value: T}[]>`. T constrained
//!   to {Number, String, Boolean} primitive set for v0.5 MVP
//!   (parity with Promise.all's existing T support).
//!
//! Heterogeneous T-tuples per spec are deferred until
//! PromiseId interning preserves per-element T shape.
//! Trailing args[1..] silent-drop per ES §27.2.4.{1,2,3,5}.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch within a matched method, `None` when the
//! callee isn't one of the 4 names on the `Promise`
//! namespace.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    /* T-17.a (v0.5.0) — Promise.all<T>(promises: Promise<T>[])
     * → Promise<Array<T>>. Sync fast-path MVP — caller's
     * input must be all-fulfilled at call time (pending
     * elements yield a rejected outer Promise). Real
     * callback fan-in lands post-T-15.g.6 once PromiseId
     * interning preserves T element shape. */
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "all" || m_name == "race" || m_name == "any" || m_name == "allSettled")
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Promise"
    {
        // S273 — accept `>= 1` arg per ES §27.2.4.{1,3,5,2}
        // trailing-arg ignore: spec reads only the iterable
        // at args[0]; trailing slots silent-drop. ssa_lower
        // mirror evals-and-drops args[1..]; typecheck-and-
        // drop here so trailing expr internal errors surface.
        if args.is_empty() {
            return Some(Err(format!(
                "Promise.{m_name} expects 1 arg (the array of Promises), got {}",
                args.len()
            )));
        }
        for &a in &args[1..] {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        let arg_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let inner = match &arg_ty {
            Type::Array(boxed) => match &**boxed {
                Type::Promise(t_box) => (**t_box).clone(),
                // §27.2.4.1 — a mixed literal (`[Promise.resolve(1),
                // 2]`) infers Array<Any>; spec resolve-wraps
                // non-thenable elements, so the shape is legal.
                // `all` only: the runtime kernel's any-lane sibling
                // (combinator_any) decodes NaN-box slots; race /
                // any / allSettled kernels still walk raw promise
                // pointers and stay rejected here (recorded L3b
                // follow-up).
                Type::Any if m_name == "all" => Type::Any,
                other => {
                    return Some(Err(format!(
                        "Promise.{m_name}: arg must be Array<Promise<T>>, got Array<{other:?}>"
                    )));
                }
            },
            other => {
                return Some(Err(format!(
                    "Promise.{m_name}: arg must be Array<Promise<T>>, got {other:?}"
                )));
            }
        };
        /* Promise.all → Promise<T[]>; .race / .any →
         * Promise<T>; .allSettled → Promise<{status,
         * value}[]>. T-17.c-A3 — allSettled accepts T
         * from the {Number, String, Bool} primitive set
         * (parity with Promise.all's existing T
         * support). Result struct's value field tracks
         * the inner T monomorphically — ssa_lower picks
         * up the field type via the returned Type::Struct
         * and emits the matching field drop (str_drop
         * for String; no-op for Number/Bool which are
         * i64-inline). Heterogeneous T-tuples per spec
         * are deferred until PromiseId interning. */
        let result = match m_name.as_str() {
            "all" => Type::Promise(Box::new(Type::Array(Box::new(inner)))),
            "allSettled" => {
                if !matches!(inner, Type::Number | Type::String | Type::Boolean) {
                    return Some(Err(format!(
                        "Promise.allSettled: T must be Number, String, or Boolean in v0.5 MVP (got {inner:?}); spec-strict heterogeneous-T shape ships post-PromiseId interning"
                    )));
                }
                Type::Promise(Box::new(Type::Array(Box::new(Type::Struct(vec![
                    ("status".to_string(), Type::String),
                    ("value".to_string(), inner.clone()),
                ])))))
            }
            _ => Type::Promise(Box::new(inner)), // race / any
        };
        return Some(Ok(result));
    }
    None
}
