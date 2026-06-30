//! Array splice / toSpliced 2-arg-undef arity narrow wedge
//! extracted from [`crate::check_type_of_call::check`]'s
//! post-cascade generic-call mechanics (chunk 301 —
//! ninety-third sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S237 — Per ES §23.1.3.31 step 7:
//! `ToIntegerOrInfinity(undefined) = 0`, so
//! `arr.splice(start, undefined)` /
//! `arr.toSpliced(start, undefined)` semantically yields
//! 0 removal (matches bun: returns `[]`, leaves source
//! unchanged). Distinct from the omitted-arg 1-arg shape
//! (chunk 300's `_array_splice_narrow`) which spec-defaults
//! deleteCount to `len - actualStart`.
//!
//! Trim both `params` and `effective_args` to the 1-arg
//! shape when arg 1 is Undefined so the strict-equality
//! arity check that follows accepts it; the
//! `ssa_lower_splice` mirror detects the 2-arg-undef shape
//! and emits `ConstI64(0)` for deleteCount instead of the
//! 1-arg `i64::MAX` sentinel.
//!
//! Mutator-shape sibling: returns `Result<(), String>` and
//! takes `effective_args: &mut Vec<ExprId>` (distinct from
//! chunk 300 which read-only borrows) so the wedge can
//! truncate the arg vector as well.
//!
//! Returns:
//! - `Ok(())` — wedge applied (params + effective_args
//!   truncated) or did not fire (arity != 2, callee not a
//!   Member, m_name outside splice/toSpliced allowlist,
//!   receiver not Array, or arg 1 not Undefined)
//! - `Err(_)` — `type_of` failed on receiver or arg 1

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &mut Vec<ExprId>,
    params: &mut Vec<Type>,
) -> Result<(), String> {
    if effective_args.len() != 2 || params.len() != 2 {
        return Ok(());
    }
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Ok(());
    };
    if !matches!(name.as_str(), "splice" | "toSpliced") {
        return Ok(());
    }
    let recv_ty = checker.type_of(ast, *obj)?;
    let arg1_ty = checker.type_of(ast, effective_args[1])?;
    if matches!(recv_ty, Type::Array(_)) && matches!(arg1_ty, Type::Undefined) {
        params.truncate(1);
        effective_args.truncate(1);
    }
    Ok(())
}
