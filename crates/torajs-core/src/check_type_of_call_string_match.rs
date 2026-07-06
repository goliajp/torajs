//! `s.{match,matchAll}(re, ...trailing)` String-receiver
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 263 — fifty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S286 — String.{match,matchAll}(re, ...trailing)
//! trailing-arg ignore per ES §22.1.3.{11,13}. Spec reads
//! only `re`; tora's regex helper takes only (Str, RegExp)
//! so trailing operands typecheck-and-drop here, ssa_lower
//! mirrors with lower-and-drop in the RegExp-branch
//! match/matchAll arms.
//!
//! Returns:
//! - `Some(Ok(Type::Array<Array<String>>))` for matchAll
//!   when receiver is String AND args[0] is RegExp AND
//!   args.len() >= 2
//! - `Some(Ok(Type::Array<String>))` for match under the
//!   same gates
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {match, matchAll}, args.len() < 2, non-String receiver,
//!   or args[0] not RegExp — cascade falls through to the
//!   copyWithin/fill / general method dispatch arms below)

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
    if !matches!(m_name.as_str(), "match" | "matchAll") || args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::RegExp) {
        return None;
    }
    for &aid in &args[1..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(if m_name == "matchAll" {
        Type::Array(Box::new(Type::Array(Box::new(Type::String))))
    } else {
        // RC-4 F1a — s.match(re) is null on miss per spec
        // §22.1.3.13 (matchAll never is); see
        // check_type_of_member_regex for the decay contract.
        Type::Nullable(Box::new(Type::Array(Box::new(Type::String))))
    }))
}
