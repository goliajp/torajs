//! `s.split(sep, limit?)` 2-arg arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 236 — twenty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — String.split accepts an optional 2nd `limit`
//! arg per JS spec §22.1.3.21. Pre-fix tora's strict 1-arg
//! signature rejected the 2-arg form; widen here.
//!
//! - **S215** — `s.split(sep, undefined)` per ES §22.1.3.21
//!   step 2: `If limit is undefined, lim = 2^32 - 1` (no
//!   truncation). Accept Undefined limit; ssa_lower mirror
//!   inline-replaces `argv[2]` with `ConstI64(i64::MAX)` so
//!   the take-min branch falls to `len`.
//!
//! Returns `Some(Ok(Array<String>))` on String-receiver match;
//! `Some(Err(_))` on bad sep / limit type; `None` otherwise
//! (non-String receiver, non-"split", or arity ≠ 2).

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
    if m_name != "split" || args.len() != 2 {
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
    // Rotation 544 — §22.1.3.23 step 3 is
    // `limit === undefined ? 2^32-1 : ToUint32(limit)`, so a
    // non-Number limit is coerced, not refused:
    // `'a,b,c'.split(',', '2')` is `['a','b']`. The undefined-vs-NaN
    // distinction that step draws is carried in ssa_lower.
    Some(Ok(Type::Array(Box::new(Type::String))))
}
