//! `prim.valueOf(...trailing)` primitive-receiver
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 260 — fifty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S290 — primitive `.valueOf(...trailing)` trailing-arg
//! ignore per ES §21.1.3.27 / §20.4.3.4 / §22.1.3.34 /
//! §21.2.3.6 / §20.5.3.5. valueOf is 0-arg spec; tora's
//! SSA-emit folds it to an identity return (recv_op)
//! without inspecting args, so trailing operands typecheck-
//! and-drop here + lower-and-drop in ssa_lower (S272
//! idiom). Covers Number / Boolean / String / BigInt /
//! Symbol receivers; Array.valueOf already handled by the
//! dedicated identity arm in ssa_lower.
//!
//! Returns `Some(Ok(<receiver-type>))` when the receiver
//! type is in the supported allow-list (Number, Boolean,
//! String, BigInt, Symbol, or Array<T>) AND m_name ==
//! "valueOf" AND args.len() >= 1 (args[..] type_of'd for
//! side effects then dropped). `Some(Err(_))` on recursive
//! `type_of` failure (receiver or any arg). `None`
//! otherwise (non-Member callee, m_name mismatch, empty
//! args, or receiver outside the allow-list — cascade
//! falls through to the pop-shift / general method
//! dispatch arms below).

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
    if m_name != "valueOf" || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let ret_ty = match src_ty {
        Type::Number => Some(Type::Number),
        Type::Boolean => Some(Type::Boolean),
        Type::String => Some(Type::String),
        Type::BigInt => Some(Type::BigInt),
        Type::Symbol => Some(Type::Symbol),
        Type::Array(ref elem) => Some(Type::Array(elem.clone())),
        _ => None,
    };
    let rt = ret_ty?;
    for &aid in args.iter() {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(rt))
}
