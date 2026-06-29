//! `xs.{reverse,toReversed,join,toString,toLocaleString}(
//! ...trailing)` Array-receiver trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 262 — fifty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S287 — Array<T>.{reverse,toReversed,join,toString,
//! toLocaleString}(...trailing) trailing-arg ignore per ES
//! §23.1.3.{27,33,15,32,31}. reverse / toReversed /
//! toString / toLocaleString are 0-arg per spec; join is
//! 1-arg (sep). tora's runtime helpers all read at most the
//! documented slots, so trailing operands typecheck-and-
//! drop here, ssa_lower mirrors with lower-and-drop in the
//! dispatch arms (S272 idiom). join's sep slot accepts
//! String | Undefined per S206; toString / toLocaleString
//! gate on the join-compatible elem types (Str/Num/Bool/Any
//! per S126-4 dispatch).
//!
//! Arity gate: `useful = m_name == "join" ? 1 : 0`. The arm
//! only fires when `args.len() > useful` (i.e. trailing-
//! widening — pure-useful shapes still hit the strict
//! method table below).
//!
//! Returns:
//! - `Some(Ok(Type::Array(<elem>)))` for {reverse,
//!   toReversed} on `Type::Array` receiver with trailing args
//! - `Some(Ok(Type::String))` for {join,toString,
//!   toLocaleString} on `Type::Array` receiver with trailing
//!   args (and arg 0 ∈ {String, Undefined} for join)
//! - `Some(Err(_))` on receiver / arg type_of failure OR on
//!   Array.join arg 0 type mismatch (exhaustive once gated
//!   to Array.join with args.len() > 1)
//! - `None` otherwise (non-Member callee, m_name mismatch,
//!   non-Array receiver, or args.len() <= useful — cascade
//!   falls through to the match/matchAll / copyWithin /
//!   general method dispatch arms below)

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
        "reverse" | "toReversed" | "join" | "toString" | "toLocaleString"
    ) {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let inner = (**elem).clone();
    let useful = if m_name == "join" { 1 } else { 0 };
    if args.len() <= useful {
        return None;
    }
    if useful == 1 {
        let sep_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(sep_ty, Type::String | Type::Undefined) {
            return Some(Err(format!(
                "Array.join arg 0 (sep) must be string, got {sep_ty:?}"
            )));
        }
    }
    for &aid in &args[useful..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "reverse" | "toReversed" => Type::Array(Box::new(inner)),
        _ => Type::String,
    }))
}
