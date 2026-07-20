//! `xs.splice(start, deleteCount, ...items)` 3+-arg insert arm
//! (RFC 20260720-splice-insert knife 2).
//!
//! The member table's static splice signature is the fixed 2-arg
//! `(Number, Number) -> Array<T>` and the narrow wedge only shrinks
//! params for FEWER args, so the ES §23.1.3.31 `...items` insert
//! form was arity-rejected outright. This wedge admits it: every
//! item must match the element type, or be `Any` when the elem is
//! Number / String (paired with `coerce_push_value`'s Any→scalar/Str
//! unbox on the lowering side — the same admit/coerce pairing as the
//! push and fill lanes; a Boolean elem has no Any-coerce arm, so Any
//! items stay rejected there rather than storing box bits).
//!
//! An `Array<Any>` receiver admits every item type except `Void`
//! (not a value type — the chunk-618 precedent): each item NaN-box
//! encodes into the 8-byte slot on the lowering side
//! (`emit_splice_items`' elem-Any arm), so there is no element type
//! to mismatch against.
//!
//! `toSpliced` (knife 3) shares the arm — same item admit, and the
//! member-table result type is the same `Array<T>` for both (the
//! removed slice for splice, the spliced clone for toSpliced).
//!
//! Returns `Some(Ok(Array<T>))` on match; `Some(Err(_))` on
//! start/deleteCount/item type mismatch; `None` otherwise
//! (non-splice/toSpliced, < 3 args, non-Array receiver).

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
    if !matches!(m_name.as_str(), "splice" | "toSpliced") || args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    let inner = (*elem).clone();
    for i in 0..2 {
        let aty = match checker.type_of(ast, args[i]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != Type::Number {
            return Some(Err(format!(
                "Array.{m_name} arg {i}: expected Number, got {aty:?}"
            )));
        }
    }
    let any_ok = matches!(inner, Type::Number | Type::String);
    for (i, aid) in args.iter().enumerate().skip(2) {
        let aty = match checker.type_of(ast, *aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(inner, Type::Any) {
            if aty == Type::Void {
                return Some(Err(format!(
                    "Array.{m_name} item {i}: Void is not a value type"
                )));
            }
            continue;
        }
        if aty != inner && !(any_ok && aty == Type::Any) {
            return Some(Err(format!(
                "Array.{m_name} item {i}: expected element type {:?}, got {aty:?}",
                inner
            )));
        }
    }
    Some(Ok(Type::Array(Box::new(inner))))
}
