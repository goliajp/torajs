//! `String.{replace,replaceAll,split}(useful, useful,
//! ...trailing)` 3+ arg trailing-arg wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 285 — seventy-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S282 — `String.{replace,replaceAll,split}(useful, useful,
//! ...trailing)` trailing-arg ignore per ES §22.1.3.{18, 19,
//! 21}. Spec reads only 2 args (search/replace pair, or
//! separator/limit pair); tora's helpers are 2-arg only.
//! Widen check.rs to accept `args.len() >= 3`; ssa_lower
//! mirror lowers `args[2..]` for side effects then drops
//! the values (S272 idiom). The `replace` and `replaceAll`
//! 1-arg-only fewer-than-2-arg case is handled by the S207
//! sibling `check_type_of_call_string_replace_undef`; the
//! `split` 2-arg basic case is handled by S?? sibling
//! `check_type_of_call_string_split_2arg`. This arm only
//! activates on `args.len() >= 3` and stays strict on the
//! String receiver.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::String` + m_name ∈ {"replace", "replaceAll",
//!   "split"} + args.len() >= 3:
//!     - typecheck-drop all args (no type constraint on
//!       useful args here — strict typing on args[0..2] is
//!       performed at the dedicated S?? / S207 wedges and
//!       the method-table dispatch site below)
//!     - returns:
//!       - `Some(Ok(Type::String))` for replace / replaceAll
//!       - `Some(Ok(Type::Array(Box::new(Type::String))))`
//!         for split
//!
//! Returns:
//! - `Some(Ok(Type::String | Type::Array(_)))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   the trio, args.len() < 3, or non-String receiver —
//!   cascade falls through to the generic callable
//!   `type_of(callee)` path)

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
    if !matches!(m_name.as_str(), "replace" | "replaceAll" | "split") {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    if let Err(e) = checker.type_of(ast, args[1]) {
        return Some(Err(e));
    }
    for &aid in &args[2..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "replace" | "replaceAll" => Type::String,
        "split" => Type::Array(Box::new(Type::String)),
        _ => unreachable!(),
    }))
}
