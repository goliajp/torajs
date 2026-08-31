//! `String.{indexOf,lastIndexOf}(needle, fromIndex?)` 2-arg
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 234 — twenty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.50 — JS spec §21.1.3.7 / §21.1.3.10. Pre-fix
//! declared with 1 fixed param; widen here.
//!
//! - **S214** — `String.indexOf(needle, undefined)` per ES
//!   §22.1.3.8 step 4: `ToIntegerOrInfinity(undefined) = 0`.
//!   ssa_lower mirror inline-replaces `argv[1]` with
//!   `ConstI64(0)`.
//! - **S216** — `String.lastIndexOf(needle, undefined)` per
//!   ES §22.1.3.10 step 5: NaN → +∞. ssa_lower lowers to
//!   `i64::MAX`; the helper clamps `from > len` to `len`.
//!
//! Returns `Some(Ok(Number))` on String-receiver match;
//! `Some(Err(_))` on bad needle / fromIndex type; `None`
//! otherwise (non-String receiver, non-{indexOf,lastIndexOf},
//! or arity ≠ 2).

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
    if !matches!(m_name.as_str(), "indexOf" | "lastIndexOf") || args.len() != 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(needle_ty, Type::String) {
        return Some(Err(format!(
            "String.{m_name} arg 0 must be string, got {needle_ty:?}"
        )));
    }
    if let Err(e) = checker.type_of(ast, args[1]) {
        return Some(Err(e));
    }
    // Rotation 544 — §22.1.3.8 step 4 / §22.1.3.10 step 5 coerce
    // this slot, so every shape reaches it: `'abc'.indexOf('c', '1')`
    // is 2 and `'abcabc'.lastIndexOf('a', 'zz')` is 3 (ToNumber of it
    // is NaN, which lastIndexOf reads as +∞). The gate that stood
    // here admitted Number and Undefined and refused the rest by
    // name. ssa_lower carries the per-method NaN reading.
    Some(Ok(Type::Number))
}
