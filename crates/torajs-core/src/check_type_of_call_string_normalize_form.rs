//! `s.normalize(form)` 1-arg optional-form wedge arm
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 248 — forty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.48 — `String.normalize` accepts an optional
//! form arg ("NFC" / "NFD" / "NFKC" / "NFKD"). Per JS spec
//! §21.1.3.13. tora's byte-Str ASCII-only path returns
//! identity for any form, so we just typecheck and route
//! through the existing 0-arg lowering.
//!
//! S208 — spec §22.1.3.13 step 1: when `form` is undefined,
//! default to "NFC". Accept the typed-Undefined arg shape
//! alongside the String form; ssa_lower routes both to the
//! existing 0-arg NFC-default path.
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::String` AND args.len() == 1 AND arg0 is
//! `Type::String | Type::Undefined`. `Some(Err(_))` on
//! arg-type mismatch (non-string non-undef) — does NOT
//! fall through; the wedge is exhaustive for this shape.
//! `None` otherwise (non-normalize callee, wrong arity, or
//! non-String receiver — cascade falls through to the
//! strict method table).

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
    if m_name != "normalize" || args.len() != 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if matches!(aty, Type::Undefined) {
        return Some(Ok(Type::String));
    }
    if !matches!(aty, Type::String) {
        return Some(Err(format!(
            "String.normalize arg must be string, got {aty:?}"
        )));
    }
    Some(Ok(Type::String))
}
