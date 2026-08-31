//! `recv.{indexOf,lastIndexOf,includes,startsWith,endsWith}(
//! needle, fromIndex, ...trailing)` String- + Array-receiver
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 265 — fifty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S278 — widen `args.len() == 3` → `>= 3` + typecheck-and-
//! drop args[2..] for any extra trailing operands per ES
//! trailing-arg ignore (same family as S270/S272/S275/S276/
//! S277). ssa_lower mirror widens the Array path gate to
//! `>= 1` + lower-and-drop args[2..]; the String path swaps
//! `break` to `let _ = lower_expr(a); continue` so step()-
//! style side-effect exprs fire per ES eval-then-discard
//! semantics. Same shape as S238 localeCompare.
//!
//! Per-method m_name allowlist:
//! - String receiver: `indexOf | lastIndexOf | includes |
//!   startsWith | endsWith` (5 methods)
//! - Array receiver: `indexOf | lastIndexOf | includes`
//!   (3 methods — startsWith/endsWith are String-only per
//!   spec)
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for {includes,startsWith,
//!   endsWith} on String, or {includes} on Array
//! - `Some(Ok(Type::Number))` for {indexOf,lastIndexOf} on
//!   either receiver
//! - `Some(Err(_))` on receiver / arg type_of failure OR on
//!   needle / fromIndex type mismatch (exhaustive once
//!   gated to String OR Array receiver with arity >= 3)
//! - `None` otherwise (non-Member callee, m_name not in
//!   the allowlist, args.len() < 3, or non-String /
//!   non-Array receiver — cascade falls through to the
//!   Object.keys / general method dispatch arms below)

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
        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
    ) || args.len() < 3
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if matches!(src_ty, Type::String) {
        let needle_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(needle_ty, Type::String | Type::Undefined) {
            return Some(Err(format!(
                "String.{m_name} arg 0 must be string, got {needle_ty:?}"
            )));
        }
        // Rotation 544 — the trailing-arg spelling carried its own
        // copy of the 2-arg gate; see the siblings for the spec
        // steps that coerce this slot.
        if let Err(e) = checker.type_of(ast, args[1]) {
            return Some(Err(e));
        }
        for &a in args.iter().skip(2) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(
            if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith") {
                Type::Boolean
            } else {
                Type::Number
            },
        ));
    }
    if let Type::Array(elem) = &src_ty
        && matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
    {
        let needle_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if needle_ty != **elem && !matches!(**elem, Type::Any) {
            return Some(Err(format!(
                "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
                **elem
            )));
        }
        let from_ty = match checker.type_of(ast, args[1]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        // Rotation 545 — the trailing spelling carried a narrower
        // gate than its 2-arg siblings (`array_index_2arg` /
        // `index_search_any` both admit Any); the lowering is shared,
        // so the two spellings must agree.
        if !matches!(from_ty, Type::Number | Type::Undefined | Type::Any) {
            return Some(Err(format!(
                "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
            )));
        }
        for &a in args.iter().skip(2) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(if m_name == "includes" {
            Type::Boolean
        } else {
            Type::Number
        }));
    }
    None
}
