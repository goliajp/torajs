//! `xs.join()` / `xs.join(undefined)` Array-receiver 0-arg
//! / 1-arg-undef-sep wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 254 — forty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.42 — Array<String|Number|Boolean|Any>.join()
//! with no sep arg defaults to ","; matches JS spec
//! §22.1.3.13. Pre-fix tora declared join with 1 fixed
//! param so `xs.join()` failed at the arity check.
//!
//! S206 — extend the same rule to an explicit `undefined`
//! sep per spec §23.1.3.16 step 1: if sep is undefined →
//! sep = ",". The 1-arg-undefined call shape was rejecting
//! at the strict-arity gate with "argument 0: expected
//! String, got Undefined".
//!
//! Returns `Some(Ok(Type::String))` when the receiver is
//! `Type::Array<String | Number | Boolean | Any>` AND args
//! is empty OR the lone arg is `Type::Undefined`.
//! `Some(Err(_))` on recursive `type_of` failure. `None`
//! otherwise (non-Member callee, m_name != "join", arg
//! present but not Undefined, or non-matching element type
//! — cascade falls through to the slice / substring /
//! general method dispatch arms below).

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
    if m_name != "join" {
        return None;
    }
    let undef_sep = if args.len() == 1 {
        let aty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        matches!(aty, Type::Undefined)
    } else {
        false
    };
    if !args.is_empty() && !undef_sep {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // The element-type list this used to carry was the `arr_join_*`
    // kernel table, not a rule of §23.1.3.18. Declining here would
    // now drop `[[1],[2]].join()` onto the 1-param member signature
    // and fail it on arity instead.
    if matches!(&src_ty, Type::Array(_)) {
        return Some(Ok(Type::String));
    }
    None
}
