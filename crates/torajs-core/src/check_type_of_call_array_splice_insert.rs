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
//! `toSpliced` is NOT admitted here — its lowering has no items
//! route yet (knife 3).
//!
//! Returns `Some(Ok(Array<T>))` (the removed slice) on match;
//! `Some(Err(_))` on start/deleteCount/item type mismatch; `None`
//! otherwise (non-splice, < 3 args, non-Array or Array<Any>
//! receiver — the latter keeps its own any-lane kernel route).

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
    if m_name != "splice" || args.len() < 3 {
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
    if matches!(inner, Type::Any) {
        return None;
    }
    for i in 0..2 {
        let aty = match checker.type_of(ast, args[i]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != Type::Number {
            return Some(Err(format!(
                "Array.splice arg {i}: expected Number, got {aty:?}"
            )));
        }
    }
    let any_ok = matches!(inner, Type::Number | Type::String);
    for (i, aid) in args.iter().enumerate().skip(2) {
        let aty = match checker.type_of(ast, *aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if aty != inner && !(any_ok && aty == Type::Any) {
            return Some(Err(format!(
                "Array.splice item {i}: expected element type {:?}, got {aty:?}",
                inner
            )));
        }
    }
    Some(Ok(Type::Array(Box::new(inner))))
}
