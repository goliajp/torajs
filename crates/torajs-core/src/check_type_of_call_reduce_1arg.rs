//! `xs.reduce(cb)` / `xs.reduceRight(cb)` 1-arg overload
//! early-route arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 215 — ninth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! ES §23.1.3.24 / §23.1.3.25: when called with a single
//! callback arg, the initial accumulator value defaults to
//! `arr[0]` (or `arr[len-1]` for reduceRight); empty array
//! throws TypeError (per spec; tr's MVP elides the guard).
//! The 2-arg form is covered by the static-sig arm. Return
//! type is the receiver's element type (consistent with the
//! 2-arg path, where the static sig pins ret = elem).
//!
//! Returns `Some(Ok(elem))` on match against an `Array<elem>`
//! receiver; `Some(Err(_))` on receiver / callback typecheck
//! failure; `None` when callee isn't `.reduce` / `.reduceRight`,
//! args.len() != 1, the receiver isn't `Array<_>`, or the CALLBACK
//! types `any` (fall through to the regular method-table dispatch —
//! for the last of those, to the any-callback arm that routes the
//! whole call to the runtime lane).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "reduce" || m_name == "reduceRight")
        && args.len() == 1
    {
        let recv_ty = match checker.type_of(ast, *recv_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if let Type::Array(elem) = &recv_ty {
            let cb_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            // An `any` callback is not this arm's to claim. Answering
            // the element type here is also a decision about the
            // LOWERING: a typed call runs the inline loop, whose
            // per-element call needs the callback's static signature,
            // and an Any operand there is `call_fn_value: expected
            // Closure or FnSig`. Declining lets the any-callback arm
            // record the callee as Any so the runtime kernel walks it
            // — which is what the 2-arg spelling has always done, and
            // why `xs.reduce(p, 0)` ran while `xs.reduce(p)` refused.
            if matches!(cb_ty, Type::Any) {
                return None;
            }
            return Some(Ok((**elem).clone()));
        }
    }
    None
}
