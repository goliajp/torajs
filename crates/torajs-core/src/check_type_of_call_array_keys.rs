//! `xs.keys(...trailing)` Array-receiver trailing-arg ignore
//! wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 259 — fifty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S292 — Array<T>.keys(...trailing) trailing-arg ignore
//! per ES §23.1.3.16. Spec sig is 0-arg returning an
//! ArrayIterator (tora: Type::ArrIter via
//! arr_iter_create_keys, fed only `recv_op`). Trailing
//! operands typecheck-and-drop here; ssa_lower mirrors with
//! lower-and-drop. values / entries stay narrow to
//! Array<Any> per the existing carve-out elsewhere in the
//! dispatch table.
//!
//! Returns `Some(Ok(Type::ArrIter))` when the receiver is
//! `Type::Array(_)` AND m_name == "keys" AND args.len() >= 1
//! (args[..] type_of'd for side effects then dropped).
//! `Some(Err(_))` on recursive `type_of` failure (receiver
//! or any arg). `None` otherwise (non-Member callee,
//! m_name mismatch, empty args, or non-Array receiver —
//! cascade falls through to the valueOf / pop-shift /
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
    if m_name != "keys" || args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Array(_)) {
        return None;
    }
    for &aid in args.iter() {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::ArrIter))
}
