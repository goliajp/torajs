//! `String.search()` / `String.search(undefined)` short-
//! circuit wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 278 — seventy-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S210 — `String.search()` / `String.search(undefined)` per
//! ES §22.1.3.20: `RegExpCreate(undefined, undefined)` yields
//! an empty regex which matches at index 0. Pre-S210 tora
//! declared `(String) -> Number`, so 0-arg failed at arity
//! and 1-arg-undefined failed with "argument 0: expected
//! String, got Undefined". ssa_lower short-circuits to
//! `ConstI64(0)`.
//!
//! Note vs sibling
//! [`crate::check_type_of_call_string_search_undef`] (chunk
//! 233): chunk 233 covers args.len() == 1 +
//! Type::Undefined needle across the
//! indexOf/lastIndexOf/includes/startsWith/endsWith/search
//! family. This wedge covers the search-specific 0-arg path
//! (chunk 233 rejects with args.len() != 1) and preserves the
//! identical 1-arg-Undefined branch byte-for-byte (dead in
//! the cascade because chunk 233 already returns earlier).
//!
//! Receiver / m_name / arity matrix:
//! - `Type::String` + m_name == "search" + args.len() < 2:
//!     - args[0] (if present) typed Undefined →
//!       `Some(Ok(Type::Number))` (dead; intercepted at
//!       chunk 233)
//!     - args.is_empty() → `Some(Ok(Type::Number))` (live)
//!     - otherwise (args[0] typed non-Undefined) → `None`
//!       cascade-fallthrough to the S324 String.search
//!       trailing-ignore wedge / general method-table arm
//!
//! Returns:
//! - `Some(Ok(Type::Number))` on success (0-arg or 1-arg-
//!   Undefined)
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name !=
//!   "search", args.len() >= 2, non-String receiver, or
//!   1-arg-non-Undefined — cascade falls through)

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
    if m_name != "search" {
        return None;
    }
    if args.len() >= 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    if let Some(&aid) = args.first() {
        let aty = match checker.type_of(ast, aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(aty, Type::Undefined) {
            return Some(Ok(Type::Number));
        }
        None
    } else {
        Some(Ok(Type::Number))
    }
}
